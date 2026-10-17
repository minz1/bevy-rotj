//! Type `$80` (bank 0 `$9297`): a hidden spawner in stage 8's area `16:00`. Every
//! 64 frames it drops a `$7E` child into the highest free main slot at a fixed y
//! `$0140`, copying its own x and `$7D90` so the child starts where the parent is.
//!
//! It calls `$A000` (`load`), `$A852` (`on_screen`), `$ABDF` (`player_dx`) and,
//! far off screen, `$CD50` (`despawn`); it never draws, integrates or contacts.
//! It dispatches on no `$7DD0` phase, so the reported phase is always 0.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// `$7E90 & $3F`: the 64-frame spawn period (`$92A1`).
const SPAWN_MASK: u8 = 0x3F;
/// The free-slot scan covers the 16 main slots, 15 down to 0 (`$92A8`).
const SLOTS: u16 = 16;
/// The child's type (`$92CC`).
const CHILD: u8 = 0x7E;
/// The child's y (`$92C5`/`$92C7` = `$01`/`$40`).
const CHILD_Y: u16 = 0x0140;
/// The child's `$7D50` (`$92CF`).
const CHILD_STATE: u8 = 0x40;
/// The child's `$7EE0` (`$92D6`).
const CHILD_CONTACT: u8 = 0x88;
/// The child's `$7D90` when the parent's has no bit 7 (`$92D4`).
const CHILD_BEHAVIOUR: u8 = 0x80;

/// Type `$80` keeps its whole state in [`Body`] (the `$7E90` spawn counter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9297`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        match engine::on_screen(work, cx.world) {
            Visibility::On => {
                if body.anim & SPAWN_MASK == 0 {
                    spawn(body, work, cx);
                }
                body.anim = body.anim.wrapping_add(1);
            }
            Visibility::Near => body.anim = body.anim.wrapping_add(1),
            Visibility::Far => engine::despawn(kind, body, 0x80, cx.schedule),
        }
    }
}

/// `$92A8`: puts a `$7E` in the highest free main slot with the parent's x.
fn spawn(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(index) = (0..SLOTS)
        .rev()
        .find(|&y| cx.slots.main[usize::from(y)].kind == 0)
    else {
        return;
    };
    let behaviour = if body.behaviour & 0x80 == 0 {
        body.behaviour
    } else {
        CHILD_BEHAVIOUR | u8::from(engine::player_dx(work, &cx.effects.player) < 0)
    };
    let child = &mut cx.slots.main[usize::from(index)];
    child.kind = CHILD;
    child.brain = Brain::new(CHILD);
    let child = &mut child.body;
    child.x = body.x;
    child.y = CHILD_Y;
    child.state = CHILD_STATE;
    child.behaviour = behaviour;
    child.contact = CHILD_CONTACT;
    child.flags = 0;
    child.phase = 0;
    child.anim = 0;
    child.vx = 0;
    child.ref_vx = 0;
    child.vx_drift = 0;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Frees the high main slots each frame, so the `$80`s' unported `$7E`
    /// children (spawned into the highest free slot) never run. The spawner
    /// places actors in slots 0-3, which are kept.
    fn free_children(scene: &mut Scene) {
        for slot in 4..16u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
    }

    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(27600) else {
            return;
        };
        free_children(&mut scene);
        let coverage = assert_actor_phases(scene, 400, &[0x80]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 400,
                occupied: 400,
                drawn: 0,
                phases: BTreeMap::from([(0, 777)]),
            },
            "{coverage:?}"
        );
    }

    /// With the parent's `$7D90` bit 7 set, the child's `$7D90` comes from
    /// `$ABDF` (`$80` when the parent is right of the player, `$81` when left).
    /// Forcing the bit and a spawn boundary each frame runs both signs.
    #[test]
    fn the_behaviour_recompute_matches_the_oracle() {
        let Some(mut scene) = Scene::tas(27600) else {
            return;
        };
        free_children(&mut scene);
        scene.isolate(&[0x80]);
        let parent = (0..16u16)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + usize::from(s)] == 0x80)
            .expect("a $80");
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7D90 + parent, 0x80));
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7E90 + parent, 0));
        let wram = &scene.machine.bus.wram;
        let px = u16::from_be_bytes([
            wram[0x1B00 + usize::from(parent)],
            wram[0x1B20 + usize::from(parent)],
        ]);
        for player in [px.wrapping_sub(0x1000), px.wrapping_add(0x1000)] {
            let mut side = scene.clone();
            let [hi, lo] = player.to_be_bytes();
            side.machine.bus.trace.pokes.push((0xD450, 0x460, hi));
            side.machine.bus.trace.pokes.push((0xD450, 0x461, lo));
            let coverage = assert_actor_phases(side, 4, &[0x80]);
            assert_eq!((coverage.occupied, coverage.drawn), (4, 0), "{coverage:?}");
        }
    }

    /// Pushed past the 128 px margin, the `$80` despawns (`$CD50`) on its first
    /// step; it never draws.
    #[test]
    fn far_off_screen_despawns() {
        let Some(mut scene) = Scene::tas(27600) else {
            return;
        };
        free_children(&mut scene);
        scene.isolate(&[0x80]);
        let slot = (0..16u16)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + usize::from(s)] == 0x80)
            .expect("a $80");
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(500 * 16).to_be_bytes();
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7B00 + slot, hi));
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7B20 + slot, lo));
        let coverage = assert_actor_phases(scene, 6, &[0x80]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 6,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            },
            "{coverage:?}"
        );
    }
}
