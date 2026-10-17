//! Type `$48` (bank 11 `$9902`): a boss-room sprite that drifts around a tall
//! rectangle and, every few laps, throws a volley of type `$4A` children.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A852`
//! (`on_screen`), `$AF38` (`free_child_slot`), `$C247` (`draw`, which `$826F`
//! maps bank 9 for) and `$A934` (`store`). It never contacts the player.
//!
//! `$7DB0` bit 7 is the path half, `$7DD0` a free-running step counter, `$7DF0`
//! bit 7 the throwing flag and `$7E10` the throw's progress. All of them live in
//! [`Body`].

use crate::actor::engine::{self, Work};
use crate::actor::families::Brain;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// Deltas, low then high byte: x low `$9A19`, x high `$9A1D`, y low `$9A21`,
/// y high `$9A25`; two entries per path half (bit 6 of `$99` picks between them).
const DRIFT_X_LOW: u16 = 0x9A19;
const DRIFT_X_HIGH: u16 = 0x9A1D;
const DRIFT_Y_LOW: u16 = 0x9A21;
const DRIFT_Y_HIGH: u16 = 0x9A25;
/// The throw-start tables, indexed by `$7DD0 >> 4`: `$9A29` facing left, `$9A39` right.
const THROW_AFTER_LEFT: u16 = 0x9A29;
const THROW_AFTER_RIGHT: u16 = 0x9A39;
/// The metasprite pointer `$9A49:$9A4A` (`$AFEA` in bank 9).
const METASPRITE: u16 = 0x9A49;
/// `$7DF0` bit 7 while a volley is in flight (`$996D`).
const THROWING: u8 = 0x80;
/// The child's type (`$9A11`) and the volley's sound (`$99CA`).
const CHILD: u8 = 0x4A;
const THROW_SOUND: u8 = 0x35;
/// `$7E10` runs 0..`$60`; children spawn every fourth step in `$20`..`$2B` (`$99B6`).
const THROW_START: u8 = 0x20;
const THROW_END: u8 = 0x2C;
const THROW_STOP: u8 = 0x60;
const SPAWN_SPACING: u8 = 4;
/// The child sits `$80` to the side and `$80` below (`$99F2`, `$9A02`): `+$80`
/// when facing left, `-$80` otherwise (`$9A4B` = `$FF/$00`).
const SPAWN_OFFSET: u16 = 0x80;
/// `$99` bit 6: facing left.
const FACING_LEFT: u8 = 0x40;
/// The `$7DB0` flip thresholds (`$9924`..`$9948`): the y low byte's bit 7 and
/// the y high byte against `4`/`5`.
const LOWER_LIMIT: u8 = 4;
const UPPER_LIMIT: u8 = 5;

/// The thrower's tables (bank 11): the path's drift and the volley's start steps.
pub struct Tables {
    /// The drift added to x, by `2 * return-leg + facing-left` (0-3) (`$9A19`, `$9A1D`).
    pub drift_x: [u16; 4],
    /// The drift added to y, by the same entry (`$9A21`, `$9A25`).
    pub drift_y: [u16; 4],
    /// Whether a volley starts at this step, facing left, by `phase >> 4` (0-15) (`$9A29`).
    pub throw_after_left: [u8; 16],
    /// Whether a volley starts at this step, facing right, by `phase >> 4` (0-15) (`$9A39`).
    pub throw_after_right: [u8; 16],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            drift_x: engine::rom_split_words(prg, HOME_BANK, DRIFT_X_HIGH, DRIFT_X_LOW),
            drift_y: engine::rom_split_words(prg, HOME_BANK, DRIFT_Y_HIGH, DRIFT_Y_LOW),
            throw_after_left: engine::rom_bytes(prg, HOME_BANK, THROW_AFTER_LEFT),
            throw_after_right: engine::rom_bytes(prg, HOME_BANK, THROW_AFTER_RIGHT),
        }
    }
}

/// Type `$48` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9902`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        if body.timer & THROWING != 0 {
            throw(body, work, cx);
        } else {
            wander(body, work, cx);
        }
        engine::on_screen(work, cx.world);
        work.tile = 0;
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, METASPRITE);
        engine::draw(work, DRAW_BANK, cx);
        engine::store(body, work);
    }
}

fn facing_left(work: &Work) -> bool {
    work.attr & FACING_LEFT != 0
}

/// Drifts along the current half of the path and swaps halves at the y limits;
/// every 16th step may begin a volley (`$9915`-`$9975`).
fn wander(body: &mut Body, work: &mut Work, cx: &Cx<'_>) {
    let tables = &cx.prg.families.volley_thrower;
    let on_return_leg = body.subtype & 0x80 != 0;
    let entry = usize::from(2 * u8::from(on_return_leg) + u8::from(facing_left(work)));
    drift(entry, work, tables);
    let [y_high, y_low] = work.y.to_be_bytes();
    if !on_return_leg {
        if y_low & 0x80 == 0 && y_high < LOWER_LIMIT {
            body.subtype = 0x80;
        }
    } else if y_low & 0x80 != 0 && y_high >= UPPER_LIMIT {
        body.subtype = 0;
    }
    body.phase = body.phase.wrapping_add(1);
    if body.phase.is_multiple_of(0x10) {
        let table = if facing_left(work) {
            &tables.throw_after_left
        } else {
            &tables.throw_after_right
        };
        if table[usize::from(body.phase >> 4)] != 0 {
            body.timer = THROWING;
            body.counter = 0;
        }
    }
}

/// Adds a drift entry to the working position (`$9992`): a 16-bit add per axis.
fn drift(entry: usize, work: &mut Work, tables: &Tables) {
    work.x = work.x.wrapping_add(tables.drift_x[entry]);
    work.y = work.y.wrapping_add(tables.drift_y[entry]);
}

/// The volley (`$99B1`): on even frames the counter runs, spawning a child every
/// fourth step from `$20` to `$2B`, and clears the flag at `$60`.
fn throw(body: &mut Body, work: &Work, cx: &mut Cx<'_>) {
    if cx.world.frame_odd {
        return;
    }
    if body.counter >= THROW_STOP {
        body.timer = 0;
        return;
    }
    if (THROW_START..THROW_END).contains(&body.counter) {
        cx.effects.sound[0] = THROW_SOUND;
        if body.counter.is_multiple_of(SPAWN_SPACING) {
            spawn_child(work, cx);
        }
    }
    body.counter = body.counter.wrapping_add(1);
}

/// Spawns the `$4A` child `$80` to the side and below (`$99D9`).
fn spawn_child(work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let facing_left = facing_left(work);
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    child.body.timer = 0;
    child.body.flags = work.attr;
    child.body.phase = u8::from(facing_left);
    child.body.x = if facing_left {
        work.x.wrapping_add(SPAWN_OFFSET)
    } else {
        work.x.wrapping_sub(SPAWN_OFFSET)
    };
    child.body.y = work.y.wrapping_add(SPAWN_OFFSET);
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The `$4A` children belong to a separate, unported family. Keeping their
    /// slots free each loop means the parent reuses the highest one and the
    /// children never run, exactly as the isolated port needs.
    fn free_children(scene: &mut Scene) {
        for slot in 16..24u16 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
    }

    /// The hinted scene: area 14:00, the two `$48` in slots 2-3, on screen for
    /// 1564 frames. `$7DD0` is a free-running counter, so the diff exercises
    /// every phase it can hold, including the throw starts at `$00`/`$50`/
    /// `$80`/`$B0` and both path halves of `$7DB0`. The handler never contacts
    /// the player, so there is no contact test.
    #[test]
    fn matches_the_oracle() {
        let Some(mut scene) = Scene::tas(26773) else {
            return;
        };
        free_children(&mut scene);
        let coverage = assert_actor_phases(scene, 1564, &[0x48]);
        assert!(coverage.occupied > 0, "{coverage:?}");
        assert!(coverage.drawn > 0, "{coverage:?}");
        let missing: Vec<u8> = (0..=0xFF)
            .filter(|phase| !coverage.phases.contains_key(phase))
            .collect();
        assert!(missing.is_empty(), "phases not reached: {missing:?}");
    }

    /// The volley really runs in this scene: the oracle puts a `$4A` in a child
    /// slot, so the diff above compares every byte of the spawn.
    #[test]
    fn the_volley_spawns_children() {
        let Some(mut scene) = Scene::tas(26773) else {
            return;
        };
        scene.isolate(&[0x48]);
        let mut spawned = false;
        for _ in 0..400 {
            scene.run(1, &[]);
            spawned |= scene.machine.bus.wram[0x1CD0..0x1CD8].contains(&0x4A);
        }
        assert!(spawned, "the oracle never spawned a $4A child");
    }
}
