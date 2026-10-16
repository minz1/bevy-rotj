//! Type `$14`, the scenery actor of stage 1-2 that falls into place (bank 8 `$8D70`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Phase 0's metasprite table (bank 8 `$8EC3`).
const PHASE0: u16 = 0x8EC3;
/// Phase 2's single metasprite (bank 8 `$8EC9`).
const PHASE2: u16 = 0x8EC9;
/// Phase 3's metasprite table (bank 8 `$8ECB`).
const PHASE3: u16 = 0x8ECB;
/// The camera-relative x pair (bank 14 `$C5F2` high, `$C6F2` low).
const X_HIGH: u16 = 0xC5F2;
const X_LOW: u16 = 0xC6F2;
/// The type `$14` child spawned at `$8EAB`.
const CHILD: u8 = 0x18;

/// Type `$14` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8D70`; phases through the table at `$8EBB`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        match body.phase {
            0 => phase0(body, work, cx),
            1 => phase1(body, cx),
            2 => phase2(kind, body, work, cx),
            3 => phase3(kind, body, work, cx),
            phase => panic!("type $14 phase {phase} is past the $8EBB table"),
        }
    }
}

/// Phase 0 (`$8D85`): draw while it falls into view, then keep the x it entered.
fn phase0(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if engine::on_screen(work, cx.world) != Visibility::On {
        return;
    }
    work.tile = 0;
    work.attr = 3;
    let frame = u16::from((body.counter >> 1) & 0xFE);
    work.pointer = engine::rom_word(cx.prg, 8, PHASE0 + frame);
    engine::draw(work, 1, cx);
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 0x0C {
        body.counter = 0x40;
        body.phase = 1;
        body.timer = work.screen_x;
    }
}

/// Phase 1 (`$8DC6`): count down, then drop from the camera plus the saved screen x.
fn phase1(body: &mut Body, cx: &Cx<'_>) {
    body.counter = body.counter.wrapping_sub(1);
    if body.counter != 0 {
        return;
    }
    body.phase = 2;
    let index = u16::from(body.timer);
    let high = engine::rom_byte(cx.prg, 14, X_HIGH + index);
    let low = engine::rom_byte(cx.prg, 14, X_LOW + index);
    body.x = cx
        .world
        .camera_x
        .wrapping_add(u16::from_be_bytes([high, low]));
    body.y = 0;
    body.behaviour = 0x40;
}

/// Phase 2 (`$8DF4`): fall and drift right until the feet find ground.
fn phase2(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    let [y_hi, y_lo] = work.y.to_be_bytes();
    let (y_lo, carry) = y_lo.overflowing_add(body.behaviour);
    work.y = u16::from_be_bytes([y_hi.wrapping_add(u8::from(carry)), y_lo]);
    body.y = work.y;
    let [x_hi, x_lo] = work.x.to_be_bytes();
    if x_hi != 0x90 {
        let (x_lo, carry) = x_lo.overflowing_add(0x09);
        work.x = u16::from_be_bytes([x_hi.wrapping_add(u8::from(carry)), x_lo]);
        body.x = work.x;
    }
    work.vy = i16::from_be_bytes([work.vy.to_be_bytes()[0], 0xFF]);
    let mut probe = Probe::default();
    if collision::feet(work, &mut probe, cx) & 0x80 == 0 {
        cx.effects.sound[0] = 0x22;
        body.phase = 3;
        body.counter = 0;
    }
    if engine::on_screen(work, cx.world) == Visibility::On {
        work.tile = 0;
        work.attr = 2;
        work.pointer = engine::rom_word(cx.prg, 8, PHASE2);
        engine::draw(work, 1, cx);
    } else {
        *kind = 0;
    }
}

/// Phase 3 (`$8E61`): sit and touch the player, then spawn a `$18` child.
fn phase3(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    work.hitbox = 0;
    if engine::on_screen(work, cx.world) != Visibility::On {
        *kind = 0;
        return;
    }
    work.tile = 0;
    work.attr = 3;
    let frame = u16::from((body.counter >> 1) & 0xFE);
    work.pointer = engine::rom_word(cx.prg, 8, PHASE3 + frame);
    engine::draw(work, 1, cx);
    engine::touch(work, 2, cx);
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 0x14 {
        *kind = 0;
        return;
    }
    if body.counter == 2 {
        // shortcut: no free slot leaves $AC16's stale Y unwritten, which no scene reaches.
        if let Some(slot) = engine::spawn_parts(4, work, cx) {
            let child = &mut cx.slots.main[slot];
            child.kind = CHILD;
            child.body.contact = 0x20;
            child.body.flags = 0x03;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actor::slots::WIDE;

    /// Moves slot `from`'s actor to `to` and frees `from`, copying the bytes `$14` reads.
    fn move_slot(scene: &mut Scene, from: usize, to: usize) {
        let wram = &mut scene.machine.bus.wram;
        for array in WIDE.into_iter().chain([0x7BA0]) {
            let at = usize::from(array - 0x6000);
            wram[at + to] = wram[at + from];
        }
        wram[0x1CC0 + to] = wram[0x1CC0 + from];
        wram[0x1CC0 + from] = 0;
    }

    /// Stage 1-2 of the TAS, keeping one `$14` in a low slot so its `$18` child
    /// (slot 15, the highest free) is stepped before it and the poke can free it.
    fn stage_1_2(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        let slots: Vec<usize> = (16..24)
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x14)
            .collect();
        let (first, rest) = slots.split_first()?;
        move_slot(&mut scene, *first, 0);
        for &slot in rest {
            scene.machine.bus.wram[0x1CC0 + slot] = 0;
        }
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CCF, 0));
        Some(scene)
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = stage_1_2(2279) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x14]);
        let expected = Coverage {
            frames: 400,
            occupied: 131,
            drawn: 63,
            phases: BTreeMap::from([(0, 11), (1, 64), (2, 36), (3, 20)]),
        };
        assert_eq!(coverage, expected);
    }

    #[test]
    fn hurts_the_player_when_it_lands() {
        let Some(mut scene) = stage_1_2(2279) else {
            return;
        };
        scene.isolate(&[0x14]);
        for _ in 0..200 {
            scene.run(1, &[]);
            if scene.machine.bus.wram[0x1DD0] == 3 {
                break;
            }
        }
        assert_eq!(scene.machine.bus.wram[0x1DD0], 3, "the $14 never lands");
        let x = u16::from_be_bytes([
            scene.machine.bus.wram[0x1B00],
            scene.machine.bus.wram[0x1B20],
        ]);
        let y = u16::from_be_bytes([
            scene.machine.bus.wram[0x1B40],
            scene.machine.bus.wram[0x1B60],
        ]);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..60 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 40, &[0x14]);
        assert!(
            coverage.phases.get(&3).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
    }
}
