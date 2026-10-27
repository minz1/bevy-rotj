//! Type `$7C`, a stage-8 walker (bank 0 `$8DFB`).
//!
//! It wakes when the player is within 80 px, drops a `$7A` marker (the shared
//! `$8D84` spawner of the `$78`/`$7C` handlers), then a `$24` blast (`$8FC0`),
//! and frees its slot once it has fallen past row `$0F`. Phases dispatch through
//! the table at `$9022`; the metasprite tables are the four words from `$902A`.

use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Counter, Metatile};
use crate::actor::world::Cx;
use crate::actors::{CELL_PENDING, STATE_UNTARGETED};

const BANK: u8 = 0;
/// The 16-bit metasprite pointers, indexed by `$7C30` (`0`/`2`/`4`).
const METASPRITES: u16 = 0x902C;
/// The spark pointer (`$902A`).
const SPARK: u16 = 0x902A;
const DRAW_BANK: u8 = 9;
const MARKER: u8 = 0x7A;
const BLAST: u8 = 0x24;
/// The wake distance, 1/16 px: `$21` (`|dx| >> 8`) must stay below 5.
const WAKE: u16 = 0x500;
const HITBOX: u8 = 0x0B;
const DAMAGE: u8 = 2;
/// The blast spawns this far to the side, and above, of the parent.
const SIDE: u16 = 0x180;
const ABOVE: u16 = 0x11F;

phases! {
    /// The walker's phase (`$7DD0`), which picks its handler (`$9022`).
    Phase {
        /// Waiting for the player to come within the wake distance.
        Waiting = 0,
        /// Dwelling for `$80` frames before the blast.
        Dwelling = 1,
        /// Drifting aside and spawning the `$24` blast.
        Blasting = 2,
        /// Falling, dropping one more `$7A` marker, until it is freed.
        Falling = 3,
    }
}

/// Type `$7C` keeps every field in [`Body`]; the pair `[subtype, behaviour]` is the y
/// velocity it wakes with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$8DFB`; phases through the table at `$9022`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        // The game's `ASL` table index drops bit 7.
        let ended = match Phase::from_byte(body.phase & 0x7F) {
            Some(Phase::Waiting) => waiting(body, work, cx),
            Some(Phase::Dwelling) => {
                dwelling(body, work);
                false
            }
            Some(Phase::Blasting) => {
                blasting(body, work, cx);
                false
            }
            Some(Phase::Falling) => falling(kind, body, work, cx),
            None => panic!("type $7C phase {} is past the $9022 table", body.phase),
        };
        if ended {
            return;
        }
        tail(kind, body, work, cx);
    }
}

/// Waiting (`$8E24`): wake when the player is near, drop the `$7A` marker, face him.
fn waiting(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    let dx = engine::player_dx(work, &cx.effects.player);
    if dx.unsigned_abs() >= WAKE {
        return true;
    }
    body.vy = i16::from_be_bytes([body.subtype, body.behaviour]);
    spawn_marker(body, body.x, cx);
    body.phase = Phase::Dwelling.byte();
    body.sprite = 0;
    body.state = 0;
    if dx >= 0 {
        work.attr |= attr::FLIP_X;
    }
    false
}

/// Dwelling (`$8E62`): a `$80`-frame dwell, then the blast.
fn dwelling(body: &mut Body, work: &Work) {
    if work.vy < 0 {
        return;
    }
    if body.timer & 0x80 == 0 {
        body.counter = 8;
        body.timer = 0x80;
        body.sprite = 2;
    } else {
        body.counter.tick_down();
        if body.counter == 0 {
            body.phase = Phase::Blasting.byte();
            body.counter = 0;
            body.timer = 0;
        }
    }
}

/// Blasting (`$8E91`): drift aside, then spawn the `$24` blast.
fn blasting(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    body.anim = 4;
    let drift = if work.attr & attr::FLIP_X != 0 {
        0x10
    } else {
        -0x10
    };
    body.vx_drift = body.vx_drift.wrapping_add(drift);
    body.sprite = 4;
    body.phase = Phase::Falling.byte();
    body.timer = 3;
    spawn_blast(body, cx);
}

/// Falling (`$8EC5`): drop one more `$7A` once it is low enough, then free the
/// slot below row `$0F`. Returns whether the slot was freed.
fn falling(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) -> bool {
    if body.timer & 0x80 == 0 && work.y.row() >= 0x0C {
        let x = work.x;
        spawn_marker(body, x, cx);
        body.timer = 0x80;
    }
    if work.y.row() >= 0x0F {
        *kind = 0;
        return true;
    }
    false
}

/// `$8D84`: puts a `$7A` in the highest free slot of 0-15 at `x`, row `$0C`,
/// and sets the caller's state to `$40`; no free slot leaves both unchanged.
fn spawn_marker(body: &mut Body, x: u16, cx: &mut Cx<'_>) {
    let Some(index) = cx.slots.free_main(0..16) else {
        return;
    };
    body.state = STATE_UNTARGETED;
    let child = cx.slots.main[index].spawn(MARKER, x, 0x0C00);
    child.phase = 0;
    child.chr_slot = 7;
}

/// `$8FC0`: puts a `$24` in the highest free slot of 16-23 beside and above the
/// parent (`$180` out, `$11F` up).
fn spawn_blast(body: &Body, cx: &mut Cx<'_>) {
    let Some(index) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let left = body.flags & attr::FLIP_X != 0;
    let x = if left {
        body.x.wrapping_sub(SIDE)
    } else {
        body.x.wrapping_add(SIDE)
    };
    let child = cx.slots.main[index].spawn(BLAST, x, body.y.wrapping_sub(ABOVE));
    child.flags = body.flags;
    child.phase = u8::from(left);
    child.chr_slot = body.chr_slot;
    child.state = 0;
    child.sprite = 5;
}

/// `$8EEE`: draws and contacts on screen, else only steps and stores; far off
/// screen it despawns with the cell re-armed.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {
            work.pointer = engine::rom_word(cx.prg, BANK, METASPRITES + u16::from(body.sprite));
            engine::tile_bits(work, cx.prg);
            engine::draw(work, DRAW_BANK, cx);
            if body.anim != 0 {
                body.anim.tick_down();
                draw_spark(work, cx);
            }
            if work.anchor & anchor::BOTTOM == 0 {
                work.hitbox = HITBOX;
                engine::contact(kind, body, work, DAMAGE, cx);
            }
        }
        Visibility::Near => {}
        Visibility::Far => {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
    }
    step_vy(body);
    engine::store(body, work);
}

/// `$8F21`: the attack spark `$18` px aside and `$1E` up; when its y would wrap
/// the x it already wrote is left behind, as the routine leaves `$94`.
fn draw_spark(work: &mut Work, cx: &mut Cx<'_>) {
    let saved_x = work.screen_x;
    let saved_y = work.screen_y;
    let x = if work.attr & attr::FLIP_X != 0 {
        match work.screen_x.checked_sub(0x18) {
            Some(x) => x,
            None => return,
        }
    } else {
        match work.screen_x.checked_add(0x18) {
            Some(x) => x,
            None => match work.screen_x.checked_sub(0x18) {
                Some(x) => x,
                None => return,
            },
        }
    };
    work.screen_x = x;
    let Some(y) = work.screen_y.checked_sub(0x1E) else {
        return;
    };
    work.screen_y = y;
    work.pointer = engine::rom_word(cx.prg, BANK, SPARK);
    let saved_attr = work.attr;
    work.attr = (work.attr & (attr::FLIP_X | attr::FLIP_Y)) | attr::PALETTE;
    engine::draw(work, DRAW_BANK, cx);
    work.screen_x = saved_x;
    work.screen_y = saved_y;
    work.attr = saved_attr;
}

/// `$8F8C`: adds 3 to the slot's y velocity and clamps its low byte to `$3F`
/// once it is positive and past `$40`.
fn step_vy(body: &mut Body) {
    body.vy = body.vy.wrapping_add(3);
    let [hi, lo] = body.vy.to_be_bytes();
    if hi & 0x80 == 0 && lo >= 0x40 {
        body.vy = i16::from_be_bytes([hi, 0x3F]);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const FRAME: u64 = 28140;

    /// The `$7A` (slots 0-15) and `$24` (slots 16-23) children are other families;
    /// freeing every main slot but slot 0 each loop keeps them from running while
    /// the parent (which the isolated spawner puts in slot 0, the lowest free one)
    /// still spawns them.
    fn free_children(scene: &mut Scene) {
        for slot in 1..24u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
    }

    /// Area 16:00, from the frame the column spawner puts a `$7C` on screen: the
    /// parent wakes, dwells, drops its `$24` blast and its second `$7A` marker,
    /// then frees its slot, and the run is long enough for a second spawn, so all
    /// four `$7DD0` phases and the draws are compared.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(FRAME) else {
            return;
        };
        free_children(&mut scene);
        let coverage = assert_actor_phases(scene, 320, &[0x7C]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 302,
                occupied: 92,
                drawn: 70,
                phases: BTreeMap::from([(0, 22), (1, 40), (2, 1), (3, 29)]),
            }
        );
    }

    /// Pinned onto the parent, the oracle takes contact damage; the diff run then
    /// compares contact with the player beside it.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(FRAME) else {
            return;
        };
        free_children(&mut scene);
        scene.isolate(&[0x7C]);
        let slot = (0..80)
            .find_map(|_| {
                scene.run(1, &[]);
                (0..16).find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x7C)
            })
            .expect("a $7C spawns");
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        // The TAS leaves `$047A` bit 6 set (the player's hurt invulnerability), which
        // gates `$A9C4`'s player check off; clear it at the loop entry as `dropper` does.
        scene.machine.bus.trace.pokes.push((0xD450, 0x047A, 0));
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..60 {
            probe.player(x, y);
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        scene.player(x, y);
        let coverage = assert_actor_phases(scene, 60, &[0x7C]);
        assert!(coverage.occupied > 0 && coverage.drawn > 0, "{coverage:?}");
    }

    /// `step_vy` over every `vy` is the byte pair it replaced.
    #[test]
    fn step_vy_is_word_maths() {
        use crate::actor::slots::Body;
        for vy in i16::MIN..=i16::MAX {
            let mut body = Body {
                vy,
                ..Body::default()
            };
            super::step_vy(&mut body);
            let [hi, lo] = vy.to_be_bytes();
            let (lo, carry) = lo.overflowing_add(3);
            let hi = hi.wrapping_add(u8::from(carry));
            let mut bytes = i16::from_be_bytes([hi, lo]);
            if hi & 0x80 == 0 && lo >= 0x40 {
                bytes = i16::from_be_bytes([hi, 0x3F]);
            }
            assert_eq!(body.vy, bytes, "{vy}");
        }
    }
}
