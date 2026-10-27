//! Type `$78`, a stage-8 controller (bank 0 `$8D4F`): while the player is above
//! row `$0C` it idles ([`WAITING`]); once he reaches that row it spawns one `$7A`
//! marker at his x and row `$0C` (the bank-0 helper `$8D84`, shared with `$7C`
//! and `$7E`), latches its phase to `$80` so it spawns only once, and frees its
//! slot when the player is in motion 7 (the level end).
//!
//! It calls no routine in `docs/ACTORS.md` §9c: it never loads, integrates,
//! draws or contacts, and it reads only the player's x (`$0460:$0461`), row
//! (`$0462`) and motion (`$046E`).

use crate::actor::engine::Work;
use crate::actor::slots::{Body, Metatile};
use crate::actor::world::{Cx, Motion};
use crate::actors::STATE_UNTARGETED;

/// The player's row that wakes the handler (`$0462` must reach `$0C`).
const ROW: u8 = 0x0C;
/// The phase latched once the marker is spawned (`$7DD0` bit 7).
const SPAWNED: u8 = 0x80;
/// The type the `$8D84` helper spawns.
const MARKER: u8 = 0x7A;

/// The phase while the player is above row `$0C`: no marker spawned yet.
const WAITING: u8 = 0;
/// Type `$78` keeps its whole state in [`Body`] (`$7DD0` only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8D4F`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        let player = cx.effects.player;
        if player.y.row() < ROW {
            body.phase = WAITING;
            return;
        }
        // `bmi`: the `$80` latch is the "already spawned" flag, so the handler
        // does nothing on later frames.
        if body.phase & SPAWNED != 0 {
            return;
        }
        body.phase = SPAWNED;
        spawn_marker(body, player.x, cx);
        if player.motion == Motion::Dying {
            *kind = 0;
        }
    }
}

/// `$8D84`: puts a `$7A` in the highest free main slot of 0-15 at `x`, row
/// `$0C`, CHR `$07`, and sets the caller's state to `$40`; no free slot leaves
/// both unchanged.
fn spawn_marker(body: &mut Body, x: u16, cx: &mut Cx<'_>) {
    let Some(index) = cx.slots.free_main(0..16) else {
        return;
    };
    body.state = STATE_UNTARGETED;
    let child = cx.slots.main[index].spawn(MARKER, x, 0x0C00);
    child.phase = 0;
    child.chr_slot = 0x07;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 16:00, from the level-start record: the player never reaches row `$0C`
    /// over the record's life, so the handler only idles at phase 0, never draws
    /// and never spawns.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(27520) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x78]);
        let expected = Coverage {
            frames: 167,
            occupied: 167,
            drawn: 0,
            phases: BTreeMap::from([(0, 167)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }

    /// The wake path: forcing the player's row to `$0C` at the actor loop makes the
    /// handler spawn its one `$7A` marker and latch phase `$80` on the first frame,
    /// then idle at `$80` while the marker runs and frees off screen.
    #[test]
    fn spawns_the_marker_at_row_c() {
        let Some(mut scene) = Scene::tas(27520) else {
            return;
        };
        // `$D450` is the actor loop's `JSR $CED5`; the poke lands before the lift.
        scene.machine.bus.trace.pokes.push((0xD450, 0x0462, 0x0C));
        let coverage = assert_actor_phases(scene, 60, &[0x78]);
        let expected = Coverage {
            frames: 27,
            occupied: 27,
            drawn: 15,
            phases: BTreeMap::from([(0x80, 26), (0x00, 1)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }

    /// The level-end arm: with the player in motion 7 on the spawn frame, the
    /// handler drops its marker and then frees its own slot, so it occupies only
    /// that one loop.
    #[test]
    fn frees_its_slot_when_the_level_ends() {
        let Some(mut scene) = Scene::tas(27520) else {
            return;
        };
        scene.machine.bus.trace.pokes.push((0xD450, 0x0462, 0x0C));
        scene.machine.bus.trace.pokes.push((0xD450, 0x046E, 0x07));
        let coverage = assert_actor_phases(scene, 60, &[0x78]);
        let expected = Coverage {
            frames: 27,
            occupied: 1,
            drawn: 15,
            phases: BTreeMap::from([(0x00, 1)]),
        };
        assert_eq!(coverage, expected, "{coverage:?}");
    }
}
