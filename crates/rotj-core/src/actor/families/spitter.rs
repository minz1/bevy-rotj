//! Type `$8A` (bank 0 `$9762`): an enemy that idles, then spits a `$8C` child.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A56E`
//! (`dying`), `$ABDF` (`player_dx`), `$A852` (`on_screen`), `$AF38`
//! (`free_child_slot`), `$AC06` (`tile_bits`), `$C247` (`draw`, through `$84CA`'s
//! bank 9 mapping), `$A9C4` (`contact`), `$CD50` (`despawn`) and `$A934`
//! (`store`). It never integrates.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// The metasprite pointers, indexed by `$7C30` (bank 0 `$9893`).
const METASPRITES: u16 = 0x9893;
/// The child's phase by the low two bits of the RNG (bank 0 `$989F`).
const CHILD_PHASES: u16 = 0x989F;
const SPRITE_BANK: u8 = 9;
const CHILD: u8 = 0x8C;
const HITBOX: u8 = 0x0B;
const DAMAGE: u8 = 2;
const WAKE_DISTANCE_HIGH_BYTE: u8 = 0x0A;
const SPIT_FRAME: u8 = 1;
const END_FRAME: u8 = 0x10;
const REST_AFTER_SPIT: u8 = 0x30;
const REST_AFTER_SPIT_WITH_BEHAVIOUR: u8 = 0x18;
const FACING_LEFT: u8 = 0x40;
/// The phase (`$7DD0`) in which it idles until the player comes near.
const WAITING: u8 = 0;
/// The phase (`$7DD0`) in which it spits a child.
const SPITTING: u8 = 1;

/// Type `$8A` keeps its phase, rest timer and spit counter in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9762`; it waits, then spits).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        if body.phase == WAITING {
            wait(body, work, cx);
        } else {
            spit(body, cx);
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
        work.pointer = engine::rom_word(cx.prg, 0, METASPRITES + u16::from(body.sprite));
        work.attr |= 0x20;
        engine::tile_bits(work, cx.prg);
        engine::draw(work, SPRITE_BANK, cx);
        if work.anchor & 0x40 == 0 {
            work.hitbox = HITBOX;
            engine::contact(kind, body, work, DAMAGE, cx);
        }
        engine::store(body, work);
    }
}

/// `$976F`: the idle sprite follows the frame counter; the rest timer runs down,
/// then the player coming within range starts a spit.
fn wait(body: &mut Body, work: &Work, cx: &Cx<'_>) {
    body.sprite = (cx.world.frame & 0x04) >> 1;
    let distance_high_byte = engine::player_dx(work, &cx.effects.player)
        .unsigned_abs()
        .to_be_bytes()[0];
    if body.timer != 0 {
        body.timer -= 1;
    } else if distance_high_byte < WAKE_DISTANCE_HIGH_BYTE {
        body.phase = SPITTING;
        body.anim = 0;
    }
}

/// `$97A6`: the spit. The child leaves on the first count; the sprite then
/// cycles until the count reaches `$10`.
fn spit(body: &mut Body, cx: &mut Cx<'_>) {
    body.anim = body.anim.wrapping_add(1);
    if body.anim == SPIT_FRAME {
        match engine::free_child_slot(cx.slots) {
            Some(slot) => spawn_child(body, slot, cx),
            None => body.anim = 0,
        }
    }
    if body.anim == END_FRAME {
        body.phase = WAITING;
        body.sprite = 0;
    } else {
        body.sprite = ((body.anim & 0x0C) >> 1) + 4;
    }
}

/// `$9822`: the `$8C` child goes into main slot `slot` (16-23), one tile ahead
/// of the parent's facing.
fn spawn_child(body: &mut Body, slot: usize, cx: &mut Cx<'_>) {
    let [x_hi, x_lo] = body.x.to_be_bytes();
    let x_hi = if body.flags & FACING_LEFT == 0 {
        x_hi.wrapping_add(1)
    } else {
        x_hi.wrapping_sub(1)
    };
    let rng = cx.world.rng[0];
    let child_phase = engine::rom_byte(cx.prg, 0, CHILD_PHASES + u16::from(rng & 3));
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    child.body.flags = body.flags;
    child.body.x = u16::from_be_bytes([x_hi, x_lo]);
    child.body.y = body.y;
    child.body.chr_slot = body.chr_slot;
    child.body.state = 0;
    child.body.phase = 0;
    child.body.timer = 0;
    child.body.behaviour = body.behaviour;
    child.brain = Brain::new(CHILD);
    body.timer = REST_AFTER_SPIT;
    if body.behaviour != 0 {
        child.body.subtype = rng & 3;
        child.body.phase = child_phase;
        body.timer = REST_AFTER_SPIT_WITH_BEHAVIOUR;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const SCENE_FRAME: u64 = 12099;

    fn scene_without_children() -> Option<Scene> {
        let mut scene = Scene::tas(SCENE_FRAME)?;
        for child_slot in 0x7CD0..0x7CD8 {
            scene.machine.bus.trace.pokes.push((0xD450, child_slot, 0));
        }
        Some(scene)
    }

    fn first_slot(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x8A)
            .expect("a live $8A")
    }

    fn only_this(scene: &mut Scene, keep: usize) {
        let wram = &mut scene.machine.bus.wram;
        for slot in (0..16).filter(|&slot| slot != keep) {
            if wram[0x1CC0 + slot] == 0x8A {
                wram[0x1CC0 + slot] = 0;
            }
        }
    }

    /// Clears the player's projectiles each loop so the TAS cannot kill the actor.
    fn clear_weapons(scene: &mut Scene) {
        for weapon in 0..16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7BD0 + weapon, 0));
        }
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene_without_children() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 360, &[0x8A]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 360,
                occupied: 77,
                drawn: 77,
                phases: BTreeMap::from([(0, 38), (1, 39)]),
            },
            "{coverage:?}"
        );
    }

    #[test]
    fn spits_a_plain_child_without_a_behaviour() {
        let Some(mut scene) = scene_without_children() else {
            return;
        };
        let slot = first_slot(&scene);
        only_this(&mut scene, slot);
        clear_weapons(&mut scene);
        let wram = &mut scene.machine.bus.wram;
        wram[0x1D90 + slot] = 0;
        wram[0x1DD0 + slot] = 1;
        wram[0x1E90 + slot] = 0;
        let coverage = assert_actor_phases(scene, 40, &[0x8A]);
        assert!(
            coverage.phases.get(&1).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
        assert_eq!(coverage.drawn, coverage.occupied, "{coverage:?}");
    }

    #[test]
    fn the_hit_flash_and_the_death_match_the_oracle() {
        let Some(mut scene) = scene_without_children() else {
            return;
        };
        let slot = first_slot(&scene);
        only_this(&mut scene, slot);
        scene.machine.bus.wram[0x1D70 + slot] = 0x80;
        let coverage = assert_actor_phases(scene, 24, &[0x8A]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    #[test]
    fn off_screen_frees_the_slot() {
        let Some(mut scene) = scene_without_children() else {
            return;
        };
        let slot = first_slot(&scene);
        only_this(&mut scene, slot);
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(0x4000).to_be_bytes();
        let wram = &mut scene.machine.bus.wram;
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        let coverage = assert_actor_phases(scene, 10, &[0x8A]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    #[test]
    fn spits_a_child_with_a_behaviour() {
        let Some(mut scene) = scene_without_children() else {
            return;
        };
        let slot = first_slot(&scene);
        only_this(&mut scene, slot);
        clear_weapons(&mut scene);
        let wram = &mut scene.machine.bus.wram;
        wram[0x1D90 + slot] = 0x80;
        wram[0x1DD0 + slot] = 1;
        wram[0x1E90 + slot] = 0;
        let coverage = assert_actor_phases(scene, 40, &[0x8A]);
        assert!(
            coverage.phases.get(&1).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
        assert_eq!(coverage.drawn, coverage.occupied, "{coverage:?}");
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = scene_without_children() else {
            return;
        };
        let slot = first_slot(&scene);
        only_this(&mut scene, slot);
        clear_weapons(&mut scene);
        scene.machine.bus.trace.pokes.push((0xD450, 0x0473, 0));
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(60, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..30 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 30, &[0x8A]);
        assert!(coverage.occupied > 0 && coverage.drawn > 0, "{coverage:?}");
    }
}
