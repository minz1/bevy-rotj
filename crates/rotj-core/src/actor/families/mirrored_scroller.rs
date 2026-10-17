//! Type `$16` (bank 8 `$8F77`): a stage controller that draws one metasprite
//! twice, mirrored 128 px apart, at screen row `$EF`, and counts `$7D90` down.
//! It calls no engine routine but the draw (`$C247`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const POINTER: u16 = 0x8FC2;
const DRAW_BANK: u8 = 1;
const ROW: u8 = 0xEF;
const ATTR: u8 = 0x02;
const MIRROR: u8 = 0x80;
const STEP: u8 = 0x0C;

/// Type `$16` keeps its x position in [`Body::behaviour`] (`$7D90`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8F77`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.tile = 0;
        work.anchor = 0;
        work.attr = ATTR;
        work.pointer = engine::rom_word(cx.prg, 8, POINTER);
        work.screen_x = body.behaviour;
        work.screen_y = ROW;
        engine::draw(work, DRAW_BANK, cx);
        work.screen_x ^= MIRROR;
        engine::draw(work, DRAW_BANK, cx);
        body.behaviour = body.behaviour.wrapping_sub(STEP);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(15677) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x16]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 27,
                occupied: 27,
                drawn: 27,
                phases: BTreeMap::from([(0, 27)]),
            }
        );
    }
}
