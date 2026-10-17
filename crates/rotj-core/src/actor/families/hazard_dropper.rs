//! Type `$A4` (bank 12 `$846A`): a dormant patrol that drops type `$A2`
//! children (`$84C2`) from three x positions — it counts `$7E90` down from `$18`
//! and drops when it underflows — and frees itself when the player is 384 px or
//! more away.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$ABDF`
//! (`player_dx`), `$CD50` (`despawn`) and `$AF38` (`free_child_slot`). It never
//! draws and never reads `$7DD0`/`$7D50`, so its phase is always the spawn value.

use crate::actor::engine::{self, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// The dropped child's type (`$84E0`).
const CHILD: u8 = 0xA2;
/// The `$7E10` value a turn-around resets to (`$84B4`).
const FLIP_COUNTER: u8 = 1;
/// The `$7E10` value that turns the patrol around (`$84A6`).
const TURN: u8 = 3;
/// The `$7E90` countdown's reload value (`$84B7`).
const DROP_PERIOD: u8 = 0x18;
/// The high byte of |dx| at or past which the slot is freed (`$8472`).
const FAR_HIGH_BYTE: u8 = 0x18;

/// Type `$A4`'s tables (bank 12), by the `$7E10` position index `counter` (`0`-`2`).
pub struct Tables {
    /// The drop's x high byte, by `counter` (`$84BF`).
    pub x_highs: [u8; 3],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            x_highs: engine::rom_bytes(prg, 12, 0x84BF),
        }
    }
}

/// Type `$A4` keeps all its state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$846A`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let dx = engine::player_dx(work, &cx.effects.player).unsigned_abs();
        if dx.to_be_bytes()[0] >= FAR_HIGH_BYTE {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
        body.anim = body.anim.wrapping_sub(1);
        if body.anim & 0x80 == 0 {
            return;
        }
        body.x = u16::from_be_bytes([
            cx.prg.families.hazard_dropper.x_highs[usize::from(body.counter)],
            0,
        ]);
        spawn_child(body, cx);
        if body.timer & 0x80 != 0 {
            body.counter = body.counter.wrapping_sub(1);
            if body.counter & 0x80 != 0 {
                flip(body);
            }
        } else {
            body.counter = body.counter.wrapping_add(1);
            if body.counter >= TURN {
                flip(body);
            }
        }
        body.anim = DROP_PERIOD;
    }
}

/// `$84C2`: a type `$A2` in the highest free child slot at the parent's x/y.
fn spawn_child(body: &Body, cx: &mut Cx<'_>) {
    let Some(slot) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    child.body.x = body.x;
    child.body.y = body.y;
    child.body.state = 0;
    child.brain = Brain::new(CHILD);
}

/// `$84AA`: turn the patrol around and re-index from the far position.
fn flip(body: &mut Body) {
    body.timer ^= 0x80;
    body.counter = FLIP_COUNTER;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Area 1A:02, slot 2: the whole 200-frame life of the `$A4`. It is never on
    /// screen itself, so every drawn loop (79) is one of its `$A2` children.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(32745) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0xA4]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 200,
                occupied: 200,
                drawn: 79,
                phases: BTreeMap::from([(0, 200)]),
            },
            "{coverage:?}"
        );
    }

    /// The 25-frame drop (`$84C2`): a live `$A2` in slots 16-23 the oracle can see.
    #[test]
    fn drops_a_child() {
        let Some(mut scene) = Scene::tas(32745) else {
            return;
        };
        scene.isolate(&[0xA4]);
        assert!((16..24).all(|slot| scene.machine.bus.wram[0x1CC0 + slot] != 0xA2));
        scene.run(25, &[]);
        assert!(
            (16..24).any(|slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xA2),
            "the $A4 dropped no child in 25 frames"
        );
    }

    /// The far check (`$8472`): the player $2000 away frees the slot (`$CD50`).
    #[test]
    fn frees_itself_when_the_player_is_far() {
        let Some(mut scene) = Scene::tas(32745) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B02], wram[0x1B22]]);
        let y = u16::from_be_bytes([wram[0x1B42], wram[0x1B62]]);
        scene.player(x.wrapping_add(0x2000), y);
        let coverage = assert_actor_phases(scene, 6, &[0xA4]);
        assert_eq!((coverage.occupied, coverage.drawn), (1, 0), "{coverage:?}");
        assert!(coverage.frames > 1, "{coverage:?}");
    }
}
