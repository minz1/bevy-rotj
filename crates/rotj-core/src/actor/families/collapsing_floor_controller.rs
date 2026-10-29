//! Type `$84`, an area-1C stage controller (bank 0 `$9361`). It counts phases
//! and, through the shared `$93E1` helper, queues two blank nametable runs and a
//! second-class `$0E` effect slot at the camera's top-right; every 16th frame it
//! drops a type `$86` through `$AC16`. It never draws and never contacts the
//! player.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$C0C6`
//! (`Effects::vram.push`) and `$AC16` (`engine::spawn_parts`). The helper
//! `$93E1` is part of the handler.

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, contact};
use crate::actor::world::{Cx, Motion};
use crate::levelmap::Cell;
use crate::render::ppu;

/// The map page the helper clears two cells of (`$6C00`).
const MAP_PAGE: u16 = 0x6C00;
/// The phase a successful first helper call leaves.
const READY_PHASE: u8 = 0x02;
/// `$7E10`'s armed value after a successful first helper call.
const ARMED: u8 = 0x80;
/// `$7DF0`'s value the first helper call arms.
const TIMER: u8 = 0x18;
/// The `$F1` period for the `$86` drop: every 16th frame.
const DROP_PERIOD: u8 = 0x10;
/// The dropped child's type.
const CHILD_KIND: u8 = 0x86;
/// `$AC16`'s variant for the drop.
const CHILD_VARIANT: u8 = 6;
/// The effect slot kind the helper installs.
const EFFECT_KIND: u8 = 0x0E;
/// The helper's `$0181` gate: it only runs once the queue is at least this full.
const QUEUE_READY: u8 = 0x70;

/// Type `$84`'s tables (bank 0).
pub struct Tables {
    /// The map offset, by `behaviour >> 4` (`$94BA`).
    pub map_table: [u8; 16],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            map_table: engine::rom_bytes(prg, 0, 0x94BA),
        }
    }
}

/// Type `$84` keeps its whole state in [`Body`] (`$7D90` behaviour, `$7DB0`
/// subtype, `$7DD0` phase, `$7DF0` timer, `$7E10` counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9361`; the `$7DD0` phase counts 0-2).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if cx.effects.player.motion == Motion::Dying {
            *kind = 0;
            return;
        }
        if cx.world.camera_x.to_be_bytes()[0] == 0 {
            cx.effects.animated = 0;
            *kind = 0;
            return;
        }
        if body.counter == 0 {
            body.timer = TIMER;
            if helper(body, cx) & 0x80 == 0 {
                body.counter = ARMED;
                body.phase = READY_PHASE;
            }
            return;
        }
        [body.phase, body.subtype] = u16::from_be_bytes([body.phase, body.subtype])
            .wrapping_add(0x0B)
            .to_be_bytes();
        if body.phase >= READY_PHASE {
            helper(body, cx);
        }
        if cx.world.frame.is_multiple_of(DROP_PERIOD) {
            work.x = cx.world.camera_x.wrapping_add(0x0380);
            work.y = 0x0C80;
            // shortcut: no free main slot leaves $AC16's stale Y unwritten, as in
            // the game; no scene reaches it.
            if let Some(slot) = engine::spawn_parts(CHILD_VARIANT, work, cx) {
                let child = &mut cx.slots.main[slot];
                child.kind = CHILD_KIND;
                child.body.contact = contact::PASSIVE;
                child.body.flags = 0x03;
            }
        }
    }
}

/// `$93E1`: if the queue is ready, places a second-class [`EFFECT_KIND`] slot at
/// the camera's top-right, blanks two nametable runs and two map cells, and ages
/// the parent. Returns the accumulator the caller branches on (`$80` when the
/// queue is not ready).
fn helper(body: &mut Body, cx: &mut Cx<'_>) -> u8 {
    if cx.effects.vram.top < QUEUE_READY {
        return 0x80;
    }
    let found = (24..32).find(|&slot| cx.slots.kind(slot) == 0);
    let Some(slot) = found else {
        // $93F8: nothing free returns the last byte the scan read ($7CC0,$1F).
        return cx.slots.kind(31);
    };
    let timer = body.timer;
    let row = ppu::tile_addr(0, 24, 0);
    cx.effects.vram.push(row | u16::from(timer), 0, &[0; 4]);
    cx.effects
        .vram
        .push(row | u16::from(timer | 0x20), 0, &[0; 4]);

    let behaviour = body.behaviour;
    let offset = cx.prg.families.collapsing_floor_controller.map_table[usize::from(behaviour >> 4)]
        | (behaviour & 0x0E);
    let cell = Cell::from_address(MAP_PAGE | u16::from(offset));
    cx.effects.level.map[cell] = 0;
    cx.effects.level.map[cell.next()] = 0;
    let camera = cx.world.camera_x.to_be_bytes()[0];
    let sprite = if (0x05..0x50).contains(&camera) {
        0x08
    } else {
        0x07
    };
    let effect = &mut cx.slots.second[slot - 24];
    effect.kind = EFFECT_KIND;
    let part = &mut effect.body;
    part.x = u16::from_be_bytes([behaviour, 0]);
    part.y = 0x0D00;
    (part.flags, part.state, part.hit, part.behaviour) = (0, 0, 0, 0);
    (part.subtype, part.phase, part.timer, part.counter) = (0, 0, 0, 0);
    (part.chr_slot, part.scratch) = (0, 0);
    part.sprite = sprite;

    body.behaviour = behaviour.wrapping_sub(2);
    body.timer = timer.checked_sub(4).unwrap_or(0x1C);
    body.phase = body.phase.wrapping_sub(2);

    sprite
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1C:00 from the level-start record in slot 0. The `$86` it drops every
    /// 16th frame lands in the highest free main slot (15), so the poke frees it
    /// at the loop head.
    fn scene() -> Option<Scene> {
        let mut scene = Scene::tas(33598)?;
        scene.machine.bus.trace.pokes.push((0xD450, 0x7CCF, 0));
        Some(scene)
    }

    /// Every `$7DD0` phase the handler produces, and the `$93E1` helper's long
    /// path. The inventory's `$FE` is the helper's `$7DD0 -= 2` write, which the
    /// caller overwrites with `$02` in the same run, so it never reaches a loop
    /// head; phases 0-2 are the frame-start set.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene() else {
            return;
        };
        let mut probe = scene.clone();
        probe.isolate(&[0x84]);
        let behaviour = probe.machine.bus.wram[0x1D90];
        let mut saw_effect = false;
        for _ in 0..300 {
            probe.run(1, &[]);
            let wram = &probe.machine.bus.wram;
            saw_effect |= (0x1CD8..0x1CE0).any(|at| wram[at] == 0x0E);
        }
        assert!(saw_effect, "the helper never installed a $0E effect slot");
        assert_ne!(
            behaviour, probe.machine.bus.wram[0x1D90],
            "the helper never aged the parent's $7D90"
        );

        let coverage = assert_actor_phases(scene, 300, &[0x84]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 264,
                occupied: 264,
                drawn: 0,
                phases: BTreeMap::from([(0, 137), (1, 126), (2, 1)]),
            }
        );
    }

    /// The `[phase, subtype]` count is the byte pair it replaced, and the timer reload its borrow.
    #[test]
    fn the_phase_count_and_the_timer_reload_are_word_maths() {
        for pair in 0..=u16::MAX {
            let [phase, subtype] = pair.to_be_bytes();
            let (subtype_bytes, carry) = subtype.overflowing_add(0x0B);
            let phase_bytes = phase.wrapping_add(u8::from(carry));
            let word = u16::from_be_bytes([phase, subtype])
                .wrapping_add(0x0B)
                .to_be_bytes();
            assert_eq!(word, [phase_bytes, subtype_bytes], "{pair:#06X}");
        }
        for timer in 0..=u8::MAX {
            let (next, borrow) = timer.overflowing_sub(4);
            let bytes = if borrow { 0x1C } else { next };
            assert_eq!(timer.checked_sub(4).unwrap_or(0x1C), bytes, "{timer:#04X}");
        }
    }
}
