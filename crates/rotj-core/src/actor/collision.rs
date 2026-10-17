//! Level collision (bank 14 `$C93B`, `$C99D`, `$CAFE`, `$CD5F` and their bank-13 wrappers).

use super::engine::{Work, rom_byte, rom_word};
use super::world::{Cx, orientation};
use crate::collision::Surface;

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

fn hi(value: u16) -> u8 {
    value.to_be_bytes()[0]
}

fn lo(value: u16) -> u8 {
    value.to_be_bytes()[1]
}

fn word(hi: u8, lo: u8) -> u16 {
    u16::from_be_bytes([hi, lo])
}

fn adc(a: u8, b: u8, carry: bool) -> (u8, bool) {
    let sum = u16::from(a) + u16::from(b) + u16::from(carry);
    (lo(sum), sum > 0xFF)
}

/// The collision class of the metatile under the probe (`2 * class`, as `$7340` holds it).
fn class_at(probe: Probe, cx: &Cx<'_>) -> u8 {
    let level = &cx.effects.level;
    let vertical = cx.world.orientation & orientation::VERTICAL != 0;
    let id = level.metatile(hi(probe.x), hi(probe.y), &cx.schedule.rows, vertical);
    level
        .class(id)
        .unwrap_or_else(|| cx.schedule.cells[usize::from(id - 0xD0)])
}

/// Is the probe's metatile solid for these flags (`$C93B`)?
pub fn solid(probe: &mut Probe, flags: u8, cx: &Cx<'_>) -> u8 {
    probe.flags = flags;
    let stored = class_at(*probe, cx);
    let by_flags = if flags & 0x04 == 0 { 0 } else { 0x80 };
    match Surface::from_stored(stored) {
        Some(
            Surface::Solid
            | Surface::BeltRight
            | Surface::BeltLeft
            | Surface::SolidHazard
            | Surface::Slippery,
        ) => 0x80,
        Some(Surface::HazardBehind | Surface::Behind | Surface::Open | Surface::SolidToBodies) => 0,
        Some(Surface::Crumbling | Surface::OneWay | Surface::OneWayBehind | Surface::Spikes) => {
            by_flags
        }
        None => {
            let stored = u16::from(stored);
            if rom_byte(cx.prg, 14, 0xC97D + stored) & 0x80 == 0 {
                rom_byte(cx.prg, 14, 0xC97E + stored)
            } else {
                by_flags
            }
        }
    }
}

/// Checks a wall at the probe and pushes the working position out of it (`$C99D`).
pub fn wall(work: &mut Work, probe: &mut Probe, flags: u8, cx: &mut Cx<'_>) -> u8 {
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
            work.attr |= 0x20;
            0x80
        }
        Surface::SolidHazard => {
            damage(work, 2, cx);
            push(work, *probe)
        }
        Surface::Behind | Surface::OneWayBehind => {
            work.attr |= 0x20;
            0x80
        }
        Surface::Open | Surface::Crumbling | Surface::OneWay => 0x80,
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
    work.probe_flags |= 0x80;
    player.hit = work.probe_flags;
    player.flags[0] &= 0x7F;
    cx.effects.scroll_lock = 0xFF;
    cx.effects.music = 0xFF;
    cx.scroll.gate = 0;
}

/// Sets the damage byte and the response's `$9F` bit 7 (`$CA4E`), unless `$0472` bit 7
/// says the player is already hurt this frame.
fn damage(work: &mut Work, amount: u8, cx: &Cx<'_>) {
    if cx.effects.player.flags[0] & 0x80 == 0 {
        work.damage = amount;
        work.probe_flags |= 0x80;
    }
}

/// Pushes out of a wall along the probe's direction (`$C9DA`).
fn push(work: &mut Work, probe: Probe) -> u8 {
    if probe.flags & 0x01 != 0 {
        let (a, _) = adc(lo(probe.x), 0x0F, true);
        let (a, carry) = adc(!a, 1, false);
        let (x_lo, carry) = adc(a, lo(work.x), carry);
        let (x_hi, _) = adc(0xFF, hi(work.x), carry);
        work.x = word(x_hi, x_lo | 0x0F);
        stop(work)
    } else if probe.flags & 0x02 != 0 {
        let (a, carry) = adc(!lo(probe.x), 0, true);
        let (x_lo, carry) = adc(a, lo(work.x), carry);
        work.x = word(hi(work.x).wrapping_add(u8::from(carry)), x_lo);
        stop(work)
    } else {
        let (a, carry) = adc(!lo(probe.y), 1, false);
        let (y_lo, carry) = adc(a, lo(work.y), carry);
        work.y = word(hi(work.y).wrapping_add(u8::from(carry)), y_lo);
        work.vy = 0;
        0
    }
}

/// Stops the x motion against a wall (`$CA1D`).
fn stop(work: &mut Work) -> u8 {
    work.ax = work.ref_vx;
    work.step_x = 0;
    0
}

/// Checks the floor at the probe and lands the working position on it (`$CAFE`).
pub fn floor(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    let stored = class_at(*probe, cx);
    let surface = Surface::from_stored(stored)
        .unwrap_or_else(|| panic!("floor class byte {stored:02X} is not ported"));
    match surface {
        Surface::Solid | Surface::SolidToBodies => land(work, probe, [0x00, 0x00]),
        Surface::BeltRight => land(work, probe, [0x10, 0x00]),
        Surface::BeltLeft => land(work, probe, [0xF0, 0xFF]),
        Surface::HazardBehind => {
            damage(work, 2, cx);
            work.attr |= 0x20;
            0x80
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
            work.attr |= 0x20;
            0x80
        }
        Surface::Open => 0x80,
        Surface::Crumbling => crumble(work, probe, cx),
        Surface::Spikes => {
            spikes(work, cx);
            land(work, probe, [0x00, 0x00])
        }
        Surface::OneWay | Surface::OneWayBehind => {
            if lo(probe.y) >= lo(work.vy.cast_unsigned()) {
                0x80
            } else {
                land(work, probe, [0x00, 0x00])
            }
        }
    }
}

/// Lands on a crumbling block from its upper half and starts it falling: second-class
/// type `$06` at the block, unless `$B9` bit 7 is set or slots 24-31 are full (`$CC6D`).
fn crumble(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    if lo(probe.y) & 0x80 != 0 {
        return 0x80;
    }
    bounce(work);
    let free = cx.slots.second.iter_mut().find(|second| second.kind == 0);
    if let Some(second) = free.filter(|_| cx.effects.player.moved & 0x80 == 0) {
        second.kind = 0x06;
        second.brain = super::families::SecondBrain::new(0x06);
        let body = &mut second.body;
        body.x = word(hi(probe.x) & 0xFE, 0xF0);
        body.anim = 0xF0;
        body.y = word(hi(probe.y), 0xF0);
        body.state = 0x80;
        (body.flags, body.hit, body.behaviour, body.subtype) = (0, 0, 0, 0);
        (body.phase, body.timer, body.counter) = (0, 0, 0);
    }
    settle(work, probe, [0x00, 0x00])
}

/// Lands on the metatile above the probe, bouncing off a hard landing (`$CB39-$CB7F`).
fn land(work: &mut Work, probe: &mut Probe, landing: [u8; 2]) -> u8 {
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

/// Puts the working position on the metatile above the probe (`$CB68-$CB7F`).
fn settle(work: &mut Work, probe: &mut Probe, landing: [u8; 2]) -> u8 {
    probe.landing = landing;
    work.y = word(hi(work.y).wrapping_sub(1), 0xFF);
    work.vy = 0;
    0
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
        if hi(dx) >= 0x10 || lo(dx >> 4) >= rom_byte(cx.prg, 14, 0xCE7D + u16::from(body.scratch)) {
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
    if work.probe_flags & 0x40 != 0 && platform.hit != 0 && platform.hit & 0x80 == 0 {
        return false;
    }
    work.y = word(hi(platform.y).wrapping_sub(1), lo(platform.y));
    work.vy = word(platform.behaviour, platform.hit).cast_signed();
    probe.landing = [platform.subtype, platform.phase];
    bounce(work);
    true
}

/// Probes the floor at the working position taken screen-relative, on pages 16-31 (`$CE3E`).
fn stand_on_screen(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    let camera = cx.world.camera_y;
    work.y = work.y.wrapping_sub(camera) | 0x3000;
    probe.x = work.x;
    probe.y = word(hi(work.y), lo(probe.y));
    floor(work, probe, cx);
    work.y = word(hi(work.y) & 0x0F, lo(work.y))
        .wrapping_add(camera)
        .wrapping_sub(1);
    true
}

/// The floor under the working position (`$A7DD`).
pub fn feet(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    probe.x = work.x;
    probe.y = work.y;
    floor(work, probe, cx)
}

/// The wall half a metatile above the working position (`$A7F0`).
pub fn head(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> u8 {
    probe.x = work.x;
    probe.y = work.y.wrapping_sub(0x80);
    wall(work, probe, 0xC0, cx)
}

/// The wall `gap` ahead in the direction of the x step (`$A80A`), probing as `column` does.
pub fn ahead(work: &mut Work, probe: &mut Probe, gap: u8, cx: &mut Cx<'_>) -> u8 {
    if work.step_x == 0 {
        return 0x80;
    }
    let direction = if work.step_x < 0 {
        probe.x = probe.x.wrapping_sub(u16::from(gap));
        2
    } else {
        probe.x = probe.x.wrapping_add(u16::from(gap));
        1
    };
    column(work, probe, direction, cx)
}

/// The wall at the probe, then one and two units higher (`$A837`, direction in `$2E`).
pub fn column(work: &mut Work, probe: &mut Probe, direction: u8, cx: &mut Cx<'_>) -> u8 {
    for raise in 0..3 {
        if raise > 0 {
            probe.y = word(hi(probe.y), lo(probe.y).wrapping_sub(1));
        }
        let result = wall(work, probe, direction, cx);
        if result & 0x80 == 0 || raise == 2 {
            return result;
        }
    }
    unreachable!("the third probe returns")
}

/// The floor at both feet when falling, else the ceiling at both shoulders (`$A95E`).
pub fn corners(work: &mut Work, probe: &mut Probe, rise: u16, half: u16, cx: &mut Cx<'_>) -> u8 {
    let falling = work.vy >= 0;
    probe.y = if falling {
        work.y
    } else {
        work.y.wrapping_sub(rise)
    };
    for x in [work.x.wrapping_add(half), work.x.wrapping_sub(half)] {
        probe.x = x;
        let result = if falling {
            floor(work, probe, cx)
        } else {
            wall(work, probe, 0x08, cx)
        };
        if result & 0x80 == 0 || x == work.x.wrapping_sub(half) {
            return result;
        }
    }
    unreachable!("the second probe returns")
}

/// Is the metatile 6/16 px below the working position solid (`$AF7D`)?
pub fn below(work: &Work, probe: &mut Probe, cx: &Cx<'_>) -> u8 {
    probe.x = work.x;
    probe.y = work.y.wrapping_add(6);
    solid(probe, 0x84, cx)
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
        A(u8),
        Riding(bool),
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
                    Answer::A(a) => r.a = Some(a),
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
            Answer::A(r.in_slot(|_, _, cx| solid(probe, flags, cx)))
        });
    }

    #[test]
    fn wall_matches_c99d() {
        tour((0xC99D, 14), &WALL_EXITS, 1000, |r, probe| {
            let flags = r.begin.cpu.a;
            Answer::A(r.in_slot(|_, work, cx| wall(work, probe, flags, cx)))
        });
    }

    #[test]
    fn floor_matches_cafe() {
        tour((0xCAFE, 14), &FLOOR_EXITS, 500, |r, probe| {
            Answer::A(r.in_slot(|_, work, cx| floor(work, probe, cx)))
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
            Answer::A(r.in_slot(|_, work, cx| feet(work, probe, cx)))
        });
    }

    #[test]
    fn head_matches_a7f0() {
        tour((0xA7F0, 13), &WALL_EXITS, 50, |r, probe| {
            Answer::A(r.in_slot(|_, work, cx| head(work, probe, cx)))
        });
    }

    #[test]
    fn ahead_matches_a80a() {
        tour((0xA80A, 13), &[0xA84E, 0xA851], 50, |r, probe| {
            let gap = r.begin.cpu.a;
            Answer::A(r.in_slot(|_, work, cx| ahead(work, probe, gap, cx)))
        });
    }

    #[test]
    fn column_matches_a837() {
        tour((0xA837, 13), &[0xA84E], 10, |r, probe| {
            let direction = r.begin.ram[0x2E];
            Answer::A(r.in_slot(|_, work, cx| column(work, probe, direction, cx)))
        });
    }

    #[test]
    fn corners_match_a95e() {
        tour((0xA95E, 13), &[0x96C1, 0x991F, 0x963D], 30, |r, probe| {
            let rise = u16::from_be_bytes([r.begin.cpu.y, r.begin.cpu.a]);
            let half = u16::from_be_bytes([r.begin.ram[0x31], r.begin.ram[0x30]]);
            Answer::A(r.in_slot(|_, work, cx| corners(work, probe, rise, half, cx)))
        });
    }

    #[test]
    fn below_matches_af7d() {
        let Some(mut scene) = Scene::tas(2500) else {
            return;
        };
        let checked = assert_routine(&mut scene, 100, (0xAF7D, 13), &[0xC973, 0xC97C], |r| {
            let mut probe = Probe::lift(&r.begin.ram);
            let a = r.in_slot(|_, work, cx| below(work, &mut probe, cx));
            r.a = Some(a);
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
            r.a = Some(r.in_slot(|_, work, cx| wall(work, &mut probe, flags, cx)));
            r.writes = probe.writes();
        });
        assert!(checked > 10, "{checked} calls");
    }
}
