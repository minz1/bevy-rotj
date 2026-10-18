//! Level collision (bank 14 `$C93B`, `$C99D`, `$CAFE`, `$CD5F` and their bank-13 wrappers).

use super::engine::{Work, hi, lo, probe_flags, rom_byte, rom_word};
use super::oam::attr;
use super::world::{Cx, orientation, power_up, sound};
use crate::collision::Surface;

/// The probe flag bits (`$24`) the ported handlers read; the other bits are the game's own.
pub mod probe {
    /// The probe looks rightward: a wall pushes back left.
    pub const RIGHTWARD: u8 = 0x01;
    /// The probe looks leftward: a wall pushes back right.
    pub const LEFTWARD: u8 = 0x02;
    /// One-way, one-way-behind, crumbling and spike tiles count as solid.
    pub const FLOORS_SOLID: u8 = 0x04;
}

/// A collision probe: point `$20-$23`, flags `$24`, landing bytes `$27/$28`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Probe {
    /// Level x, 1/16 px (`$21:$20`).
    pub x: u16,
    /// Level y, 1/16 px (`$23:$22`).
    pub y: u16,
    /// Probe flags (`$24`).
    pub flags: u8,
    /// Landing bytes (`$27`, `$28`).
    pub landing: [u8; 2],
}

/// The collision class of the metatile under the probe (`2 * class`, as `$7340` holds it).
fn class_at(probe: Probe, cx: &Cx<'_>) -> u8 {
    let level = &cx.effects.level;
    let vertical = cx.world.orientation & orientation::VERTICAL != 0;
    let id = level.metatile(
        hi(probe.x),
        hi(probe.y),
        &cx.schedule.page_columns,
        vertical,
    );
    level
        .class(id)
        .unwrap_or_else(|| cx.schedule.cells[usize::from(id - 0xD0)])
}

/// Is the probe's metatile solid for these flags (`$C93B`)?
/// True when solid.
pub fn solid(probe: &mut Probe, flags: u8, cx: &Cx<'_>) -> bool {
    probe.flags = flags;
    let stored = class_at(*probe, cx);
    let by_flags = flags & probe::FLOORS_SOLID != 0;
    match Surface::from_stored(stored) {
        Some(
            Surface::Solid
            | Surface::BeltRight
            | Surface::BeltLeft
            | Surface::SolidHazard
            | Surface::Slippery,
        ) => true,
        Some(Surface::HazardBehind | Surface::Behind | Surface::Open | Surface::SolidToBodies) => {
            false
        }
        Some(Surface::Crumbling | Surface::OneWay | Surface::OneWayBehind | Surface::Spikes) => {
            by_flags
        }
        None => {
            let stored = u16::from(stored);
            if rom_byte(cx.prg, 14, 0xC97D + stored) & 0x80 == 0 {
                rom_byte(cx.prg, 14, 0xC97E + stored) & 0x80 != 0
            } else {
                by_flags
            }
        }
    }
}

/// Checks a wall at the probe and pushes the working position out of it (`$C99D`).
/// True when a wall blocked.
pub fn wall(work: &mut Work, probe: &mut Probe, flags: u8, cx: &mut Cx<'_>) -> bool {
    probe.flags = flags;
    let stored = class_at(*probe, cx);
    let surface = Surface::from_stored(stored)
        .unwrap_or_else(|| panic!("wall class byte {stored:02X} is not ported"));
    match surface {
        Surface::Solid
        | Surface::BeltRight
        | Surface::BeltLeft
        | Surface::Slippery
        | Surface::SolidToBodies => push(work, *probe),
        Surface::HazardBehind => {
            damage(work, 2, cx);
            work.attr |= attr::BEHIND;
            false
        }
        Surface::SolidHazard => {
            damage(work, 2, cx);
            push(work, *probe)
        }
        Surface::Behind | Surface::OneWayBehind => {
            work.attr |= attr::BEHIND;
            false
        }
        Surface::Open | Surface::Crumbling | Surface::OneWay => false,
        Surface::Spikes => {
            spikes(work, cx);
            push(work, *probe)
        }
    }
}

/// Spikes (`$CAA9` in a wall, `$CD09` underfoot): unless `$B9` bit 7 is set, damage
/// `$10` with `$9F` bit 7 (kept in `$A8`), `$0472` bit 7 cleared, `$C2` and the music
/// `$FF`, the auto-scroll gate `$B6` shut.
fn spikes(work: &mut Work, cx: &mut Cx<'_>) {
    let player = &mut cx.effects.player;
    if player.moved & 0x80 != 0 {
        return;
    }
    work.damage = 0x10;
    work.probe_flags |= probe_flags::DAMAGED;
    player.hit = work.probe_flags;
    player.power_up &= !power_up::ON;
    cx.effects.scroll_lock = 0xFF;
    cx.effects.music = sound::music::STOP;
    cx.scroll.gate = 0;
}

/// Sets the damage byte and the response's `$9F` bit 7 (`$CA4E`), unless `$0472` bit 7
/// says the power-up is on.
fn damage(work: &mut Work, amount: u8, cx: &Cx<'_>) {
    if cx.effects.player.power_up & power_up::ON == 0 {
        work.damage = amount;
        work.probe_flags |= probe_flags::DAMAGED;
    }
}

/// Pushes out of a wall along the probe's direction (`$C9DA`); always true (blocked).
fn push(work: &mut Work, probe: Probe) -> bool {
    if probe.flags & probe::RIGHTWARD != 0 {
        let a = lo(probe.x).wrapping_add(0x10);
        work.x = work.x.wrapping_add(0xFF00).wrapping_add(back(a)) | 0x0F;
        stop(work)
    } else if probe.flags & probe::LEFTWARD != 0 {
        work.x = work.x.wrapping_add(back(lo(probe.x)));
        stop(work)
    } else {
        work.y = work.y.wrapping_add(back(lo(probe.y)));
        work.vy = 0;
        true
    }
}

/// The ROM's negate of `a`, whose carry (set only for 0) leaks into the next add.
fn back(a: u8) -> u16 {
    u16::from(a.wrapping_neg()) + u16::from(a == 0)
}

/// Stops the x motion against a wall (`$CA1D`); always true (blocked).
fn stop(work: &mut Work) -> bool {
    work.ax = work.ref_vx;
    work.step_x = 0;
    true
}

/// Checks the floor at the probe and lands the working position on it (`$CAFE`).
/// True when it landed.
pub fn floor(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    let stored = class_at(*probe, cx);
    let surface = Surface::from_stored(stored)
        .unwrap_or_else(|| panic!("floor class byte {stored:02X} is not ported"));
    match surface {
        Surface::Solid | Surface::SolidToBodies => land(work, probe, [0x00, 0x00]),
        Surface::BeltRight => land(work, probe, [0x10, 0x00]),
        Surface::BeltLeft => land(work, probe, [0xF0, 0xFF]),
        Surface::HazardBehind => {
            damage(work, 2, cx);
            work.attr |= attr::BEHIND;
            false
        }
        Surface::SolidHazard => {
            damage(work, 2, cx);
            land(work, probe, [0x00, 0x00])
        }
        Surface::Slippery => {
            work.ax = if work.age.trailing_zeros() >= 5 {
                (work.ax.wrapping_sub(work.ref_vx) >> 2).wrapping_neg()
            } else {
                0
            };
            settle(work, probe, [0x00, 0x00])
        }
        Surface::Behind => {
            work.attr |= attr::BEHIND;
            false
        }
        Surface::Open => false,
        Surface::Crumbling => crumble(work, probe, cx),
        Surface::Spikes => {
            spikes(work, cx);
            land(work, probe, [0x00, 0x00])
        }
        Surface::OneWay | Surface::OneWayBehind => {
            if lo(probe.y) >= lo(work.vy.cast_unsigned()) {
                false
            } else {
                land(work, probe, [0x00, 0x00])
            }
        }
    }
}

/// The falling block's second-class kind (`crumbling_block` family).
const CRUMBLING_BLOCK: u8 = 0x06;

/// Lands on a crumbling block from its upper half and starts it falling: second-class
/// type `$06` at the block, unless `$B9` bit 7 is set or slots 24-31 are full (`$CC6D`).
/// True when it landed.
fn crumble(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    if lo(probe.y) & 0x80 != 0 {
        return false;
    }
    bounce(work);
    let free = cx.slots.second.iter_mut().find(|second| second.kind == 0);
    if let Some(second) = free.filter(|_| cx.effects.player.moved & 0x80 == 0) {
        second.kind = CRUMBLING_BLOCK;
        second.brain = super::families::SecondBrain::new(CRUMBLING_BLOCK);
        let body = &mut second.body;
        body.x = (probe.x & 0xFE00) | 0xF0;
        body.anim = 0xF0;
        body.y = (probe.y & 0xFF00) | 0xF0;
        body.state = 0x80;
        (body.flags, body.hit, body.behaviour, body.subtype) = (0, 0, 0, 0);
        (body.phase, body.timer, body.counter) = (0, 0, 0);
    }
    settle(work, probe, [0x00, 0x00])
}

/// Lands on the metatile above the probe, bouncing off a hard landing (`$CB39-$CB7F`);
/// always true (landed).
fn land(work: &mut Work, probe: &mut Probe, landing: [u8; 2]) -> bool {
    bounce(work);
    settle(work, probe, landing)
}

/// Halves the x speed against the reference and reverses it after six frames (`$CB39-$CB66`).
fn bounce(work: &mut Work) {
    if work.age < 6 {
        work.ax = 0;
    } else {
        work.age = 0;
        work.ax = (work.ax.wrapping_sub(work.ref_vx) >> 1).wrapping_neg();
    }
}

/// Puts the working position on the metatile above the probe (`$CB68-$CB7F`); always
/// true (landed).
fn settle(work: &mut Work, probe: &mut Probe, landing: [u8; 2]) -> bool {
    probe.landing = landing;
    work.y = (work.y & 0xFF00).wrapping_sub(1);
    work.vy = 0;
    true
}

/// Lands on a second-class actor just under the working position (`$CD5F`); true when
/// it stood on one (the N flag clear, which is what the callers branch on).
pub fn ride(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    if work.vy < 0 {
        return false;
    }
    for index in (0..8).rev() {
        let second = &cx.slots.second[index];
        if second.kind == 0 {
            continue;
        }
        let body = &second.body;
        let dx = body.x.abs_diff(work.x);
        let half_width = cx.prg.hitboxes.platform_half_widths[usize::from(body.scratch)];
        if hi(dx) >= 0x10 || lo(dx >> 4) >= half_width {
            continue;
        }
        let (dy, above) = body.y.overflowing_sub(work.y);
        if above || hi(dy) >= 0x10 || lo(dy >> 4) >= 0x10 {
            continue;
        }
        cx.slots.set_pressed(index, 0x80);
        let body = cx.slots.second[index].body.clone();
        return match rom_word(cx.prg, 14, 0xCE79 + u16::from(body.counter)) {
            0xCDD9 => stand(work, probe, &body),
            0xCE3E => stand_on_screen(work, probe, cx),
            other => panic!("riding handler ${other:04X} is not ported"),
        };
    }
    false
}

/// Stands on a platform and takes its motion (`$CDD9`); false when it slips off.
fn stand(work: &mut Work, probe: &mut Probe, platform: &super::slots::Body) -> bool {
    if work.probe_flags & probe_flags::BLOCKED_Y != 0
        && platform.hit != 0
        && platform.hit & 0x80 == 0
    {
        return false;
    }
    work.y = platform.y.wrapping_sub(0x100);
    work.vy = i16::from_be_bytes([platform.behaviour, platform.hit]);
    probe.landing = [platform.subtype, platform.phase];
    bounce(work);
    true
}

/// Probes the floor at the working position taken screen-relative, on pages 16-31 (`$CE3E`).
fn stand_on_screen(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    let camera = cx.world.camera_y;
    work.y = work.y.wrapping_sub(camera) | 0x3000;
    probe.x = work.x;
    probe.y = (work.y & 0xFF00) | (probe.y & 0xFF);
    floor(work, probe, cx);
    work.y = (work.y & 0x0FFF).wrapping_add(camera).wrapping_sub(1);
    true
}

/// The floor under the working position (`$A7DD`); true when it landed.
pub fn feet(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    probe.x = work.x;
    probe.y = work.y;
    floor(work, probe, cx)
}

/// The wall half a metatile above the working position (`$A7F0`); true when it blocked.
pub fn head(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    probe.x = work.x;
    probe.y = work.y.wrapping_sub(0x80);
    wall(work, probe, 0xC0, cx)
}

/// The wall `gap` ahead in the direction of the x step (`$A80A`), probing as `column` does.
/// True when a wall blocked.
pub fn ahead(work: &mut Work, probe: &mut Probe, gap: u8, cx: &mut Cx<'_>) -> bool {
    if work.step_x == 0 {
        return false;
    }
    let direction = if work.step_x < 0 {
        probe.x = probe.x.wrapping_sub(u16::from(gap));
        probe::LEFTWARD
    } else {
        probe.x = probe.x.wrapping_add(u16::from(gap));
        probe::RIGHTWARD
    };
    column(work, probe, direction, cx)
}

/// The wall at the probe, then one and two units higher (`$A837`, direction in `$2E`).
/// True when a wall blocked.
pub fn column(work: &mut Work, probe: &mut Probe, direction: u8, cx: &mut Cx<'_>) -> bool {
    for raise in 0..3 {
        if raise > 0 {
            // The low byte only, no borrow.
            probe.y = u16::from_be_bytes([hi(probe.y), lo(probe.y).wrapping_sub(1)]);
        }
        let blocked = wall(work, probe, direction, cx);
        if blocked || raise == 2 {
            return blocked;
        }
    }
    unreachable!("the third probe returns")
}

/// The floor at both feet when falling, else the ceiling at both shoulders (`$A95E`).
/// True when one landed or blocked.
pub fn corners(work: &mut Work, probe: &mut Probe, rise: u16, half: u16, cx: &mut Cx<'_>) -> bool {
    let falling = work.vy >= 0;
    probe.y = if falling {
        work.y
    } else {
        work.y.wrapping_sub(rise)
    };
    for x in [work.x.wrapping_add(half), work.x.wrapping_sub(half)] {
        probe.x = x;
        let hit = if falling {
            floor(work, probe, cx)
        } else {
            wall(work, probe, 0x08, cx)
        };
        if hit || x == work.x.wrapping_sub(half) {
            return hit;
        }
    }
    unreachable!("the second probe returns")
}

/// Is the metatile 6/16 px below the working position solid (`$AF7D`)? True when solid.
pub fn below(work: &Work, probe: &mut Probe, cx: &Cx<'_>) -> bool {
    probe.x = work.x;
    probe.y = work.y.wrapping_add(6);
    solid(probe, 0x80 | probe::FLOORS_SOLID, cx)
}

/// Rides second-class actors at the working position (`$AF6A`).
pub fn ride_here(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    probe.x = work.x;
    probe.y = work.y;
    ride(work, probe, cx)
}

#[cfg(test)]
mod layout {
    use super::Probe;

    impl Probe {
        pub(crate) fn lift(ram: &[u8; 2048]) -> Probe {
            Probe {
                x: u16::from_be_bytes([ram[0x21], ram[0x20]]),
                y: u16::from_be_bytes([ram[0x23], ram[0x22]]),
                flags: ram[0x24],
                landing: [ram[0x27], ram[0x28]],
            }
        }

        pub(crate) fn writes(self) -> Vec<(u16, u8)> {
            let [x_hi, x_lo] = self.x.to_be_bytes();
            let [y_hi, y_lo] = self.y.to_be_bytes();
            vec![
                (0x20, x_lo),
                (0x21, x_hi),
                (0x22, y_lo),
                (0x23, y_hi),
                (0x24, self.flags),
                (0x27, self.landing[0]),
                (0x28, self.landing[1]),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::oracle::{Routine, Scene, assert_routine};

    const WALL_EXITS: [u16; 6] = [0xC9F6, 0xCA2B, 0xCA9F, 0xCAA2, 0xCAA5, 0xCAA8];
    const FLOOR_EXITS: [u16; 4] = [0xCB7F, 0xCC69, 0xCC6C, 0xCD08];
    enum Answer {
        Hit(bool),
        Solid(bool),
        Riding(bool),
    }

    /// The ROM's A for a hit probe's result: 0 when it hit, else `$80`.
    fn hit_byte(hit: bool) -> u8 {
        if hit { 0 } else { 0x80 }
    }

    const RIDE_EXITS: [u16; 5] = [0xCD6D, 0xCD93, 0xCDE6, 0xCE1A, 0xCE3D];

    /// The red-brick room with the player dropped onto the moving platform (slot 24).
    fn on_platform() -> Option<Scene> {
        let mut scene = Scene::replay("stage1-inputs.csv", 1100)?;
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B18], wram[0x1B38]]);
        let y = u16::from_be_bytes([wram[0x1B58], wram[0x1B78]]);
        scene.player(x, y.wrapping_sub(0x100));
        Some(scene)
    }

    /// Route frames 641-999, a brawl at 900 and the platform: the player, `$C6`, `$22`,
    /// item drops and riding.
    fn tour(
        (entry, bank): (u16, u8),
        exits: &[u16],
        min: usize,
        mut port: impl FnMut(&mut Routine<'_>, &mut Probe) -> Answer,
    ) {
        if rotj_rom::for_test().is_none() {
            return;
        }
        let mut run = |scene: Option<Scene>, frames| {
            let Some(mut scene) = scene else {
                return 0;
            };
            assert_routine(&mut scene, frames, (entry, bank), exits, |r| {
                let mut probe = Probe::lift(&r.begin.ram);
                match port(r, &mut probe) {
                    Answer::Hit(hit) => r.a = Some(hit_byte(hit)),
                    Answer::Solid(solid) => r.negative = Some(solid),
                    Answer::Riding(riding) => r.negative = Some(!riding),
                }
                r.writes = probe.writes();
            })
        };
        let checked = run(Scene::route(640), 359)
            + run(Scene::route(900).map(|s| s.brawl(1, 0x300, 120)), 120)
            + run(on_platform(), 120);
        assert!(checked >= min, "${entry:04X}: {checked} calls");
    }

    #[test]
    fn solid_matches_c93b() {
        tour((0xC93B, 14), &[0xC973, 0xC97C], 100, |r, probe| {
            let flags = r.begin.cpu.a;
            Answer::Solid(r.in_slot(|_, _, cx| solid(probe, flags, cx)))
        });
    }

    #[test]
    fn wall_matches_c99d() {
        tour((0xC99D, 14), &WALL_EXITS, 1000, |r, probe| {
            let flags = r.begin.cpu.a;
            Answer::Hit(r.in_slot(|_, work, cx| wall(work, probe, flags, cx)))
        });
    }

    #[test]
    fn floor_matches_cafe() {
        tour((0xCAFE, 14), &FLOOR_EXITS, 500, |r, probe| {
            Answer::Hit(r.in_slot(|_, work, cx| floor(work, probe, cx)))
        });
    }

    #[test]
    fn ride_matches_cd5f() {
        tour((0xCD5F, 14), &RIDE_EXITS, 500, |r, probe| {
            Answer::Riding(r.in_slot(|_, work, cx| ride(work, probe, cx)))
        });
    }

    #[test]
    fn feet_matches_a7dd() {
        tour((0xA7DD, 13), &FLOOR_EXITS, 100, |r, probe| {
            Answer::Hit(r.in_slot(|_, work, cx| feet(work, probe, cx)))
        });
    }

    #[test]
    fn head_matches_a7f0() {
        tour((0xA7F0, 13), &WALL_EXITS, 50, |r, probe| {
            Answer::Hit(r.in_slot(|_, work, cx| head(work, probe, cx)))
        });
    }

    #[test]
    fn ahead_matches_a80a() {
        tour((0xA80A, 13), &[0xA84E, 0xA851], 50, |r, probe| {
            let gap = r.begin.cpu.a;
            Answer::Hit(r.in_slot(|_, work, cx| ahead(work, probe, gap, cx)))
        });
    }

    #[test]
    fn column_matches_a837() {
        tour((0xA837, 13), &[0xA84E], 10, |r, probe| {
            let direction = r.begin.ram[0x2E];
            Answer::Hit(r.in_slot(|_, work, cx| column(work, probe, direction, cx)))
        });
    }

    #[test]
    fn corners_match_a95e() {
        tour((0xA95E, 13), &[0x96C1, 0x991F, 0x963D], 30, |r, probe| {
            let rise = u16::from_be_bytes([r.begin.cpu.y, r.begin.cpu.a]);
            let half = u16::from_be_bytes([r.begin.ram[0x31], r.begin.ram[0x30]]);
            Answer::Hit(r.in_slot(|_, work, cx| corners(work, probe, rise, half, cx)))
        });
    }

    #[test]
    fn below_matches_af7d() {
        let Some(mut scene) = Scene::tas(2500) else {
            return;
        };
        let checked = assert_routine(&mut scene, 100, (0xAF7D, 13), &[0xC973, 0xC97C], |r| {
            let mut probe = Probe::lift(&r.begin.ram);
            r.negative = Some(r.in_slot(|_, work, cx| below(work, &mut probe, cx)));
            r.writes = probe.writes();
        });
        assert!(checked >= 1, "{checked} calls");
    }

    #[test]
    fn ride_here_matches_af6a() {
        tour((0xAF6A, 13), &RIDE_EXITS, 100, |r, probe| {
            Answer::Riding(r.in_slot(|_, work, cx| ride_here(work, probe, cx)))
        });
    }

    #[test]
    fn push_is_word_maths() {
        fn adc(a: u8, b: u8, carry: bool) -> (u8, bool) {
            let sum = u16::from(a) + u16::from(b) + u16::from(carry);
            (sum.to_be_bytes()[1], sum > 0xFF)
        }
        fn old(w: u16, p: u8, flags: u8) -> u16 {
            let [w_hi, w_lo] = w.to_be_bytes();
            if flags & probe::RIGHTWARD != 0 {
                let (a, _) = adc(p, 0x0F, true);
                let (a, carry) = adc(!a, 1, false);
                let (x_lo, carry) = adc(a, w_lo, carry);
                let (x_hi, _) = adc(0xFF, w_hi, carry);
                u16::from_be_bytes([x_hi, x_lo | 0x0F])
            } else {
                let (a, carry) = adc(!p, u8::from(flags == 0), flags != 0);
                let (lo, carry) = adc(a, w_lo, carry);
                u16::from_be_bytes([w_hi.wrapping_add(u8::from(carry)), lo])
            }
        }
        for flags in [probe::RIGHTWARD, probe::LEFTWARD, 0] {
            for hi in [0x00, 0xFF] {
                for w_lo in 0..=0xFF {
                    for p in 0..=0xFF {
                        let w = u16::from_be_bytes([hi, w_lo]);
                        let probe = Probe {
                            x: u16::from(p),
                            y: u16::from(p),
                            flags,
                            ..Probe::default()
                        };
                        let mut work = Work {
                            x: w,
                            y: w,
                            ..Work::default()
                        };
                        assert!(push(&mut work, probe));
                        let got = if flags == 0 { work.y } else { work.x };
                        assert_eq!(got, old(w, p, flags), "{flags} {w:04X} {p:02X}");
                    }
                }
            }
        }
        let pushed = |flags, p, w| {
            let mut work = Work {
                x: w,
                ..Work::default()
            };
            let probe = Probe {
                x: p,
                flags,
                ..Probe::default()
            };
            push(&mut work, probe);
            work.x
        };
        assert_eq!(pushed(probe::LEFTWARD, 0x00, 0x1230), 0x1231);
        assert_eq!(
            pushed(probe::RIGHTWARD, 0xF0, 0x1230),
            0x1230u16.wrapping_sub(0xFF) | 0x0F
        );
    }

    /// Stage 7-1's wall of spikes (id `$C5`, class `$10`, x `$BD`, rows 9-14): Batman
    /// put beside it walks into it (`$CAA9`, frame 38205).
    #[test]
    fn spikes_match_caa9() {
        let Some(mut scene) = Scene::tas(38200) else {
            return;
        };
        scene.player(0xBC00, 0x0E00);
        scene.camera(0xB500);
        scene.script(120, |i| if i < 40 { 0x80 } else { 0x81 });
        let spiked = scene.clone().run(120, &[0xCAA9]).len();
        assert_eq!(spiked, 1);
        let checked = assert_routine(&mut scene, 120, (0xC99D, 14), &WALL_EXITS, |r| {
            let mut probe = Probe::lift(&r.begin.ram);
            let flags = r.begin.cpu.a;
            r.a = Some(hit_byte(
                r.in_slot(|_, work, cx| wall(work, &mut probe, flags, cx)),
            ));

            r.writes = probe.writes();
        });
        assert!(checked > 10, "{checked} calls");
    }
}
