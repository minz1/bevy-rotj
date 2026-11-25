//! Second-class type `$0E`, a falling floor piece type `$84` drops (bank 0 `$94C3`).

use crate::sim::collision::wall;
use crate::sim::engine::Work;
use crate::sim::slots::{Body, EMPTY};
use crate::sim::world::Cx;

use super::crumbling_block::fall;

/// Falls like the crumbling block [`super::crumbling_block`] from its sixth frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame (`$94C3`): fall until a wall probe finds the level or it passes row 16.
    pub fn step(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if let Some(mut probe) = fall(body)
            && (body.y >= 0x1000 || wall(work, &mut probe, 0x80, cx))
        {
            *kind = EMPTY;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::oracle::{Scene, assert_second_phases};

    #[test]
    fn matches_the_oracle_in_stage_6_2() {
        let Some(scene) = Scene::tas(33640) else {
            return;
        };
        let coverage = assert_second_phases(scene, 600, &[0x0E]);
        assert!(coverage.occupied > 100, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
    }
}
