//! Type `$96` (bank 12 `$8065`, area 1C:00): a fixed-screen sprite the `$94`
//! spawner (`$8027`) drops into a free slot, drawn at screen (120, 117) and
//! cycling three `$7DD0` phases.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A56E`
//! (`dying`, variant `$80`), `$AF38` (`free_child_slot`), `$AC06` (`tile_bits`),
//! `$C247` (`draw`, which `$81C6` maps bank 9 at `$A000`), `$A9C4` (`contact`)
//! and `$A934` (`store`). The position it stores is a camera offset, not the
//! actor's own x/y, so it never integrates and no `$A852` projection runs.
//!
//! The spawner writes its contact byte `$50`, whose bit 6 makes `$A9C4` skip the
//! player check: the `$96` never hurts the player, only the `$AA57` weapon scan
//! acts, so a hit kills it. Its lobbing phase leaves a `$9A` child (bank 12 `$8208`,
//! unported) in slots 16-23.

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Field};
use crate::actor::world::Cx;

/// The metasprite pointer table, read in the handler's own bank (`$81B4`).
const METASPRITES: u16 = 0x81B4;
/// The bank `$81C6` maps at `$A000` around the draw.
const DRAW_BANK: u8 = 9;
/// The child type the lobbing step puts in a free slot (`$817E`).
const CHILD: u8 = 0x9A;
/// The hit box the contact uses (`$8149`).
const HITBOX: u8 = 0x0B;
/// The fixed screen position the handler stores (`$8131`/`$8135`).
const SCREEN_X: u8 = 0x78;
const SCREEN_Y: u8 = 0x75;
/// The camera offsets the working position is set to (`$8086`/`$8093`).
const CAMERA_X_OFFSET: u16 = 0x0780;
const CAMERA_Y_OFFSET: u16 = 0x0750;
/// The child's offset from the working position (`$8165`/`$8179`).
const CHILD_X_OFFSET: u8 = 0xD0;
const CHILD_Y_OFFSET: u8 = 0x02;
/// The phase each `$7DD0` counter change sets (`$80C2`, `$80DE`, `$8106`).
const NEXT_PHASE: u8 = 0x10;
const LOOP_PHASE: u8 = 0x18;
/// The child's initial phase and timer (`$818E`/`$8193`).
const CHILD_PHASE: u8 = 0xE8;
const CHILD_TIMER: u8 = 0xFF;

phases! {
    /// The sprite's phase (`$7DD0`), which picks its handler (`$81C0`).
    Phase {
        /// Appearing: stepping through its first sprite frames.
        Appearing = 0,
        /// Holding the resting sprite.
        Holding = 1,
        /// Lobbing: dropping the `$9A` child, then back to holding.
        Lobbing = 2,
    }
}

/// Type `$96` keeps its phase, animation counter and sprite frame in [`Body`]
/// (`$7DD0`, `$7E90`, `$7C30`), and its spawner's slot in `counter`: its death clears
/// the spawner's phase (`$806F`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8065`, phases at `$81C0`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            let spawner = usize::from(body.counter);
            cx.set(spawner, Field::Phase, 0);
            return;
        }
        work.attr |= body.flags;
        work.x = cx.world.camera_x.wrapping_add(CAMERA_X_OFFSET);
        work.y = cx.world.camera_y.wrapping_add(CAMERA_Y_OFFSET);
        match Phase::from_byte(body.phase) {
            Some(Phase::Appearing) => {
                body.sprite = body.anim >> 3;
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= NEXT_PHASE {
                    body.phase = Phase::Holding.byte();
                    body.anim = 0;
                }
            }
            Some(Phase::Holding) => {
                body.sprite = 2;
                body.anim = body.anim.wrapping_add(1);
                if body.anim >= NEXT_PHASE {
                    body.phase = Phase::Lobbing.byte();
                    body.anim = 0;
                }
            }
            Some(Phase::Lobbing) => {
                body.anim = body.anim.wrapping_add(1);
                if body.anim == NEXT_PHASE
                    && let Some(slot) = engine::free_child_slot(cx.slots)
                {
                    spawn_child(work, slot, cx);
                }
                if body.anim == LOOP_PHASE {
                    body.phase = Phase::Holding.byte();
                    body.sprite = 2;
                    body.anim = 2;
                } else {
                    body.sprite = ((body.anim & 0x18) >> 3) + 3;
                }
            }
            None => panic!("type $96 phase {} is past the $81C0 table", body.phase),
        }
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + 2 * u16::from(body.sprite));
        work.screen_x = SCREEN_X;
        work.screen_y = SCREEN_Y;
        if cx.oam.chr[2] == 0x0F {
            work.screen_y = work.screen_y.wrapping_add(1);
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, 0, cx);
        engine::store(body, work);
    }
}

/// `$8162`: the `$9A` child goes into main slot `slot` (16-23), `$D0` left and
/// `$200` above the working position, its phase and timer from the table, its
/// counter and animation from the RNG.
fn spawn_child(work: &Work, slot: usize, cx: &mut Cx<'_>) {
    let [x_hi, x_lo] = work.x.to_be_bytes();
    let (x_lo, borrow) = x_lo.overflowing_sub(CHILD_X_OFFSET);
    let x_hi = x_hi.wrapping_sub(u8::from(borrow));
    let [y_hi, y_lo] = work.y.to_be_bytes();
    let y_hi = y_hi.wrapping_sub(CHILD_Y_OFFSET);
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    let part = &mut child.body;
    part.x = u16::from_be_bytes([x_hi, x_lo]);
    part.y = u16::from_be_bytes([y_hi, y_lo]);
    (part.state, part.subtype, part.behaviour) = (0, 0, 0);
    (part.phase, part.timer) = (CHILD_PHASE, CHILD_TIMER);
    let [seed, noise] = cx.world.rng;
    let (counter, anim) = if seed & 0x80 == 0 {
        (noise & 0x0F, 0)
    } else {
        (noise | 0xE0, 0xFF)
    };
    cx.slots.main[slot].body.counter = counter;
    cx.slots.main[slot].body.anim = anim;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// Clears the player's projectiles and any `$9A` child each loop, so the
    /// actor lives through its whole phase cycle and the unported child never runs.
    fn without_weapons_and_children(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for weapon in 0..16u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7BD0 + weapon, 0));
        }
        for child in 0x7CD0..0x7CD8u16 {
            scene.machine.bus.trace.pokes.push((0xD450, child, 0));
        }
        Some(scene)
    }

    /// The whole phase cycle from the spawn frame: phases 0, 1 and 2, drawn in 69
    /// of 70 loops, the `$9A` child dropped when phase 2 reaches `$10`. Disabling
    /// the spawn makes this fail, so the `$8162` write is compared, not skipped.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = without_weapons_and_children(33632) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 70, &[0x96]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 70,
                occupied: 70,
                drawn: 69,
                phases: BTreeMap::from([(0, 16), (1, 30), (2, 24)]),
            },
            "{coverage:?}"
        );
    }

    /// The natural scene: the player's weapon kills it, so the `$A56E` death and
    /// the `$806F` spawner-phase clear run (the clear is visible because `isolate`
    /// leaves the freed `$94` spawner's `$7DD0` at `$80` until the `$96` dies).
    #[test]
    fn matches_the_oracle_through_its_death() {
        let Some(scene) = Scene::tas(33652) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x96]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 60,
                occupied: 15,
                drawn: 15,
                phases: BTreeMap::from([(1, 15)]),
            },
            "{coverage:?}"
        );
    }
}
