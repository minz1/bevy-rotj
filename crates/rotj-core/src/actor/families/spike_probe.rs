//! Types `$58` (bank 11 `$9E23`) and `$5A` (bank 11 `$9E7A`), the stage 5 pair: `$58`
//! probes a player-relative metatile and runs the level end, `$5A` flashes the palette
//! shadow. Both keep their whole state in [`Body`].

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::{Cx, Motion};

/// Types `$58`/`$5A`'s tables (bank 11).
pub struct Tables {
    /// `$58`'s probe y offset, by the player's motion (0-11, the death `7` aside) (`$9E6F`).
    pub motion_y_offsets: [u8; 12],
    /// `$5A`'s alternating palette, by `counter + i` (0-3 or 4-7) (`$9EBA`).
    pub flash_palettes: [u8; 8],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            motion_y_offsets: engine::rom_bytes(prg, 11, 0x9E6F),
            flash_palettes: engine::rom_bytes(prg, 11, 0x9EBA),
        }
    }
}

/// Both kinds keep no state outside [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of either handler.
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        match *kind {
            0x58 => probe(work, cx),
            0x5A => flash(body, cx),
            other => panic!("type ${other:02X} is not in the `$58`/`$5A` family"),
        }
    }
}

/// `$58`: builds the probe at the player's x and a player-relative y, calls the wall
/// check `$C99D` and runs the level end `$D274` when the probe reported damage.
fn probe(work: &mut Work, cx: &mut Cx<'_>) {
    let player = cx.effects.player;
    let motion = player.motion;
    if motion == Motion::Dying {
        return;
    }
    let [x_hi, x_lo] = player.x.to_be_bytes();
    let [y_hi, y_lo] = player.y.to_be_bytes();
    let (y_lo, borrow) = y_lo.overflowing_sub(0xC0);
    let y_hi = y_hi
        .wrapping_sub(cx.prg.families.spike_probe.motion_y_offsets[usize::from(motion.byte())])
        .wrapping_sub(u8::from(borrow));
    let index = cx.effects.scroll_y.wrapping_add(0x60);
    let mut probe = Probe {
        x: u16::from_be_bytes([x_hi, x_lo]),
        y: u16::from_be_bytes([y_hi, y_lo]).wrapping_add(u16::from(index) << 4),
        flags: 0,
        landing: [0; 2],
    };
    work.probe_flags = 0;
    collision::wall(work, &mut probe, 0x88, cx);
    if work.probe_flags & 0x80 != 0 {
        engine::level_end(cx.effects);
    }
}

/// `$5A`: counts `$7DD0` up; every eighth count, when no palette write is pending and
/// the VRAM queue has room, flips `$7E10` bit 2 and writes the ROM palette to
/// `$04E4-$04E7`. A blocked swap sets the `$7DF0` retry, which forces the swap next frame.
fn flash(body: &mut Body, cx: &mut Cx<'_>) {
    if body.timer & 0x80 != 0 {
        swap(body, cx);
        return;
    }
    body.phase = body.phase.wrapping_add(1);
    if body.phase & 0x07 != 0 {
        return;
    }
    if cx.effects.palette_dirty & 0x80 != 0 || cx.effects.vram.top < 0x78 {
        body.timer = 0x80;
        return;
    }
    swap(body, cx);
}

/// `$9E9C`: toggles the palette selector and writes the four bytes to `$04E4-$04E7`.
fn swap(body: &mut Body, cx: &mut Cx<'_>) {
    body.counter ^= 0x04;
    for i in 0..4u16 {
        let index = usize::from(body.counter) + usize::from(i);
        cx.effects.palette[4 + usize::from(i)] = cx.prg.families.spike_probe.flash_palettes[index];
    }
    cx.effects.palette_dirty = 0x80;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Type `$58` runs 192 actor-loop frames from the stage 5 room change, phase 0 and
    /// never drawn. The run stops at frame 14238, so `$58`'s level-end branch is not
    /// exercised here.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(13982) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 256, &[0x58]);
        let expected = Coverage {
            frames: 192,
            occupied: 192,
            drawn: 0,
            phases: BTreeMap::from([(0, 192)]),
        };
        assert_eq!(coverage, expected);
    }

    /// Type `$5A` counts `$7DD0` 0-64, flashing the palette at each eighth count, then
    /// holds at 64 once the `$7DF0` retry latches; it is never drawn.
    #[test]
    fn every_phase_of_the_flash_runs() {
        let Some(scene) = Scene::tas(13982) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 600, &[0x5A]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (397, 397, 0)
        );
        let expected: BTreeMap<u8, usize> = (0u8..=64)
            .map(|phase| (phase, if phase == 64 { 333 } else { 1 }))
            .collect();
        assert_eq!(coverage.phases, expected);
    }
}
