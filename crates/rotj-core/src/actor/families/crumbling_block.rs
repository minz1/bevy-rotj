//! Second-class type `$06`, a crumbling block (bank 8 `$80F5`), spawned by the floor
//! class handler `$CC6D` when the player lands on one.

use crate::actor::collision::{Probe, wall};
use crate::actor::engine::{self, TileBlock, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// `state` bit 7 while the block is still in the nametable and the map.
const UNCLEARED: u8 = 0x80;
/// Frames `state` counts before the block falls.
const WAIT: u8 = 5;

/// The block keeps its wait in `state` (bit 7 until it is cleared from the map) and its
/// fall speed in `hit` (1/16 px) and `behaviour` (px).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$80F5`): on its first, blank the block in the nametable and the map;
    /// then wait five frames and fall, accelerating, until a wall probe finds the level.
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if body.state & UNCLEARED != 0 {
            if cx.effects.vram.top >= 0x60 {
                clear(body, work, cx);
            }
            return;
        }
        if let Some(mut probe) = fall(body)
            && wall(work, &mut probe, 0x80, cx) & 0x80 == 0
        {
            *kind = 0;
        }
    }
}

/// Waits five frames, then falls, accelerating (`$8173`, also `$0E`'s `$94C3`); the
/// probe at its new position.
pub(crate) fn fall(body: &mut Body) -> Option<Probe> {
    if body.state < WAIT {
        body.state += 1;
        return None;
    }
    accelerate_fall(body);
    Some(Probe {
        x: body.x,
        y: body.y,
        ..Probe::default()
    })
}

/// Adds 3 to the fall speed (clamped at `$80`) and moves y by it (`$8173`).
pub(crate) fn accelerate_fall(body: &mut Body) {
    body.hit = body.hit.wrapping_add(3);
    if body.hit & 0x80 != 0 {
        body.hit = 0x80;
    }
    let fall_speed = u16::from_be_bytes([body.behaviour, body.hit]);
    body.y = body.y.wrapping_add(fall_speed);
}

/// Queues four blank tiles across two rows at the block and zeroes its two map cells (`$8106`).
fn clear(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    let [column, _] = body.x.to_be_bytes();
    let [row, _] = body.y.to_be_bytes();
    let block = TileBlock {
        column,
        row,
        width: 4,
        height: 2,
        tiles: &[0; 8],
        fields: None,
    };
    engine::queue_block(&block, cx);
    work.x = u16::from_be_bytes([column, work.x.to_be_bytes()[1]]);
    work.y = u16::from_be_bytes([row, work.y.to_be_bytes()[1]]);
    let (base, _) = engine::map_page(work, cx);
    let at = usize::from(row & 0x0F) * 0x100 + usize::from(base) + usize::from(column & 0x0F);
    cx.effects.level.map[at..at + 2].fill(0);
    body.state = 0;
    body.scratch = 0;
    let hit = body.hit;
    body.behaviour = hit;
    body.counter = hit;
    body.subtype = hit;
    body.phase = hit;
    body.anim = hit;
    body.chr_slot = hit;
    body.sprite = 5;
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_stage_6_1() {
        let Some(scene) = Scene::tas(32900) else {
            return;
        };
        let coverage = assert_second_phases(scene, 360, &[0x06]);
        assert!(coverage.occupied > 100, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
