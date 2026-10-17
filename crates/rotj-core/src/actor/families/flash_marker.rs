//! Type `$7A`, a stationary level-8 effect (bank 0 `$8DB5`): a 15-frame flash that
//! increments its `$7DD0` counter each frame, draws the bank-9 metasprite the
//! counter selects, and frees its own slot at phase `$10` or once off screen.
//!
//! It is put in its slot by the shared bank-0 `$8D84` spawner (`STA $7CC0,y` at
//! `$8DAC`), which its parents call: the `$7C` handler in phase 0 (mostly) and
//! phase 3, and the `$7E` handler in phase 6. The `$78` handler also calls it but
//! never spawns in the TAS, so it is not a `$7A` parent there.
//!
//! It calls only routines already in `docs/ACTORS.md` §9c: `$A000` (`load`),
//! `$A852` (`on_screen`), `$AC06` (`tile_bits`), `$C247` (`draw`) and `$A934`
//! (`store`). `$84CA` maps bank 9 at `$A000` around the draw (the port passes
//! bank 9 to `engine::draw`). No `$A79B` integrate, no `$A9C4` contact: it never
//! moves and never touches the player.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The `$96:$97` metasprite table (bank 0 `$8DF3`), indexed by `(phase & $FC) >> 1`.
const METASPRITES: u16 = 0x8DF3;
/// The bank `$84CA` maps at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The phase at which the handler frees its own slot (`$10`).
const LAST_PHASE: u8 = 0x10;

/// Type `$7A` keeps its whole state in [`Body`] (`$7DD0` only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8DB5`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = 0;
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        body.phase = body.phase.wrapping_add(1);
        if body.phase >= LAST_PHASE {
            *kind = 0;
            return;
        }
        let frame = (body.phase & 0xFC) >> 1;
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(frame));
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(27584) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 30, &[0x7A]);
        let expected = Coverage {
            frames: 30,
            occupied: 16,
            drawn: 15,
            phases: BTreeMap::from([
                (0x00, 1),
                (0x01, 1),
                (0x02, 1),
                (0x03, 1),
                (0x04, 1),
                (0x05, 1),
                (0x06, 1),
                (0x07, 1),
                (0x08, 1),
                (0x09, 1),
                (0x0A, 1),
                (0x0B, 1),
                (0x0C, 1),
                (0x0D, 1),
                (0x0E, 1),
                (0x0F, 1),
            ]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }
}
