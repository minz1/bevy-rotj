//! Type `$B6` (bank 12 `$9153`, area 22:00): a stage-7 boss-area part drawn at
//! fixed screen offsets, cycling three `$7DD0` phases and dropping a `$BA` part
//! when its spawning counter passes `$10`.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A56E`
//! (`dying`, variant 0), `$C0C6` (`Vram::push`, through its own `$9249` tail),
//! `$AF38` (`free_child_slot`), `$C247` (`draw`, which `$81C6` maps bank 9 at
//! `$A000`) and `$A9C4` (`contact`). Its draw position is the offset table, not
//! the actor's level position, so it never integrates and no `$A852` projection
//! or `$A8E4` end-of-frame runs: it returns at `$CEDE` and the slot keeps its
//! arrays.

use crate::actor::engine::{self, Work};
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, Motion};

/// The metasprite pointer table, read in the handler's own bank (`$932A`),
/// indexed by `$7C30` (0-10).
const METASPRITES: u16 = 0x932A;
/// The screen x offsets, indexed by `$7DB0` (`$9340`).
const X_OFFSETS: u16 = 0x9340;
/// The screen y offsets, indexed by `$7DB0` (`$9344`).
const Y_OFFSETS: u16 = 0x9344;
/// The bank `$81C6` maps at `$A000` around the draw.
const DRAW_BANK: u8 = 9;
/// The hit box the contact uses (`$A5` = `$0A` at `$923D`).
const HITBOX: u8 = 0x0A;
/// The child type the phase-2 counter spawn drops (`$92B3`).
const CHILD: u8 = 0xBA;
/// `$9249` reads this table of source pointers, indexed by `$7DB0` (`$9378`).
const DEATH_SOURCES: u16 = 0x9378;
/// `$9249` reads this table of destination addresses, indexed by `$7DB0` (`$9380`).
const DEATH_DESTS: u16 = 0x9380;
/// The subtype/timer pairs the spawn reads (`$938C`).
const SPAWN_TYPES: u16 = 0x938C;
/// The phase/counter pairs the spawn reads (`$93C4`).
const SPAWN_STATES: u16 = 0x93C4;
/// The queue room `$9249` needs before it runs (`$0181` >= `$70`).
const VRAM_ROOM: u8 = 0x70;

phases! {
    /// The part's phase (`$7DD0`), which picks its handler (`$9324`).
    Phase {
        /// Animating until the player's boss-entry motion ends.
        Waiting = 0,
        /// Animating for `$C0` frames until a frame boundary.
        Cycling = 1,
        /// Playing the spawn animation and dropping a `$BA` part.
        Spawning = 2,
    }
}

/// The part's constant tables (bank 12).
pub struct Tables {
    /// The screen x offsets, by `subtype` 0-3, from the spawn records (`$9340`).
    pub x_offsets: [u8; 4],
    /// The screen y offsets, by `subtype` 0-3, from the spawn records (`$9344`).
    pub y_offsets: [u8; 4],
    /// The death tiles' first nametable address, high byte first, by `subtype` 0-3 (`$9380`).
    pub death_dests: [u16; 4],
    /// The child's subtype and timer, by `child_index / 2`, 0-27 (`$938C`).
    pub spawn_types: [(u8, u8); 28],
    /// The child's phase and counter, by `child_index / 2`, 0-27 (`$93C4`).
    pub spawn_states: [(u8, u8); 28],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        let dests: [u8; 8] = engine::rom_bytes(prg, 12, DEATH_DESTS);
        let types: [u8; 56] = engine::rom_bytes(prg, 12, SPAWN_TYPES);
        let states: [u8; 56] = engine::rom_bytes(prg, 12, SPAWN_STATES);
        Tables {
            x_offsets: engine::rom_bytes(prg, 12, X_OFFSETS),
            y_offsets: engine::rom_bytes(prg, 12, Y_OFFSETS),
            death_dests: std::array::from_fn(|i| {
                u16::from_be_bytes([dests[2 * i], dests[2 * i + 1]])
            }),
            spawn_types: std::array::from_fn(|i| (types[2 * i], types[2 * i + 1])),
            spawn_states: std::array::from_fn(|i| (states[2 * i], states[2 * i + 1])),
        }
    }
}

/// Type `$B6` keeps its whole state in [`Body`] (`$7DD0` phase, `$7E90` counter,
/// `$7E10` latch, `$7C30` sprite frame).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9153`, phases at `$9324`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if body.health & 0x8000 != 0 {
            // Dying: the first frame (dying timer 0) shifts the slot down $0540
            // and queues its death tiles, if the nametable queue has room.
            if body.dying_timer == 0 {
                if cx.effects.vram.top < VRAM_ROOM {
                    return;
                }
                body.y = body.y.wrapping_add(0x0540);
                queue_death_tiles(body, cx);
            }
            engine::dying(kind, body, work, 0, cx);
            return;
        }
        match Phase::from_byte(body.phase) {
            Some(Phase::Waiting) => {
                body.sprite = (cx.world.frame >> 2) & 3;
                if cx.effects.player.motion != Motion::BossEntry {
                    body.phase = Phase::Cycling.byte();
                    body.anim = 0;
                }
            }
            Some(Phase::Cycling) => {
                body.sprite = (cx.world.frame >> 2) & 3;
                body.anim.tick_up();
                if body.anim >= 0xC0 && cx.world.frame & 0x0C == 0 {
                    body.phase = Phase::Spawning.byte();
                    body.anim = 0;
                    body.counter = 0;
                }
            }
            Some(Phase::Spawning) => {
                if body.counter != 0 {
                    body.sprite = ((body.anim & 0x1C) >> 2) + 4;
                    body.anim.tick_up();
                    if body.anim >= 0x1C {
                        body.phase = Phase::Cycling.byte();
                        body.anim = 0;
                    }
                } else {
                    body.sprite = ((body.anim & 0x0C) >> 2) + 4;
                    body.anim.tick_up();
                    if body.anim >= 0x10 {
                        body.counter = 0x80;
                        spawn_child(body, work, cx);
                    }
                }
            }
            None => panic!("type $B6 phase {} is past the $9324 table", body.phase),
        }
        // `$9209`: the draw, then contact. `$94`-`$99` are set here, `$98`
        // (CHR slot) is whatever `load` carried.
        work.tile = 0;
        work.attr = 0;
        work.pointer = engine::rom_word(cx.prg, 12, METASPRITES + 2 * u16::from(body.sprite));
        let tables = &cx.prg.families.final_boss_part;
        let facing = usize::from(body.subtype);
        work.screen_x = tables.x_offsets[facing];
        work.screen_y = tables.y_offsets[facing];
        if body.sprite >= 8 {
            work.screen_y = work.screen_y.wrapping_sub(8);
        }
        engine::draw(work, DRAW_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, 0, cx);
    }
}

/// `$9249`: queues four rows of four tiles from the source table at the
/// destination addresses, each row `$20` on, then latches the dying timer.
fn queue_death_tiles(body: &mut Body, cx: &mut Cx<'_>) {
    let prg = cx.prg;
    let facing = 2 * u16::from(body.subtype);
    let source = engine::rom_word(prg, 12, DEATH_SOURCES + facing);
    let mut addr = prg.families.final_boss_part.death_dests[usize::from(body.subtype)];
    for row in 0..4u16 {
        let tiles: [u8; 4] = std::array::from_fn(|i| {
            engine::rom_byte(prg, 12, source + 4 * row + u16::try_from(i).expect("small"))
        });
        cx.effects.vram.push(addr, 0, &tiles);
        addr = addr.wrapping_add(0x20);
    }
    body.dying_timer = 1;
}

/// `$92AB`: puts a `$BA` part in the highest free slot 16-23 at the working
/// position, its subtype/timer and phase/counter from the tables at an index
/// the parent's facing and the player's x pick.
fn spawn_child(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let Some(slot) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let index = child_index(body, cx);
    let tables = &cx.prg.families.final_boss_part;
    let pair = usize::from(index >> 1);
    let (subtype, timer) = tables.spawn_types[pair];
    let (phase, counter) = tables.spawn_states[pair];
    let child = cx.slots.main[slot].spawn(CHILD, work.x, work.y);
    (child.state, child.anim) = (0, 0);
    (child.subtype, child.timer) = (subtype, timer);
    (child.phase, child.counter) = (phase, counter);
}

/// The `$938C`/`$93C4` index (`$9305`): the parent's `$7DB0` doubled plus the
/// player's x high byte mapped through the `$92EA` clamp.
fn child_index(body: &Body, cx: &Cx<'_>) -> u16 {
    let x_hi = cx.effects.player.x.to_be_bytes()[0];
    let shifted = (x_hi & 0x0E) << 2;
    let offset = if shifted < 0x20 {
        let (value, borrow) = shifted.overflowing_sub(8);
        if borrow { 0 } else { value }
    } else if shifted < 0x38 {
        shifted
    } else {
        0x30
    };
    u16::from(body.subtype.wrapping_shl(1).wrapping_add(offset))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};
    use crate::actor::spawn::SpawnTable;

    /// Every `$B6` record's `subtype` indexes the subtype tables, and no `$68` spawns a `$B6`.
    #[test]
    fn every_subtype_is_in_the_tables() {
        let Some(prg) = crate::prg::for_test() else {
            return;
        };
        let bound = prg.families.final_boss_part.x_offsets.len();
        let mut checked = 0;
        for (area, room) in crate::rooms::listed_rooms(prg) {
            for record in SpawnTable::import(prg, area, room).records.iter().flatten() {
                assert!(
                    record.kind != 0x68 || record.subtype != 0xB6,
                    "area {area} room {room}"
                );
                if record.kind == 0xB6 {
                    assert!(
                        usize::from(record.subtype) < bound,
                        "area {area} room {room}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no $B6 record");
    }

    /// Frees every `$BA` child slot each loop, so the `$BA` never runs.
    fn without_children(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for child in 0x7CD0..0x7CD8u16 {
            scene.machine.bus.trace.pokes.push((0xD450, child, 0));
        }
        Some(scene)
    }

    /// Area 22:00 from the level-start record (frame 39680): the four `$B6` slots
    /// run their whole lives, every phase, drawn every loop, then the
    /// player's weapon kills them at 39913 and the dying tail runs. Pinning the
    /// phases proves each arm of the `$9324` dispatch and the spawning drop run.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = without_children(39680) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 420, &[0xB6]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 400,
                occupied: 400,
                drawn: 400,
                phases: BTreeMap::from([(0, 400), (1, 941), (2, 84)]),
            }
        );
    }

    /// The spawning drop is non-vacuous: a `$BA` part lands in a free child slot
    /// at a running `$B6`'s position, with its `$7D50`/`$7E90` cleared.
    #[test]
    fn the_phase_two_spawn_drops_a_ba_part() {
        let Some(mut scene) = without_children(39680) else {
            return;
        };
        scene.isolate(&[0xB6]);
        let events = scene.run(420, &[0xD450, 0xD453]);
        let spawn = events
            .windows(2)
            .find(|window| {
                (window[0].pc, window[1].pc) == (0xD450, 0xD453)
                    && (0x1CD0..0x1CD8).any(|addr| window[1].wram[addr] == 0xBA)
            })
            .expect("spawning drops a $BA part");
        let (begin, end) = (&spawn[0], &spawn[1]);
        // The child slot (16-23) is the `$7CD0` byte's index.
        let child = (0x1CD0..0x1CD8usize)
            .find(|&addr| end.wram[addr] == 0xBA)
            .expect("a $BA child")
            - 0x1CC0;
        let x = u16::from_be_bytes([end.wram[0x1B00 + child], end.wram[0x1B20 + child]]);
        let positions: Vec<u16> = (0..16)
            .filter(|&slot| begin.wram[0x1CC0 + slot] == 0xB6)
            .map(|slot| u16::from_be_bytes([begin.wram[0x1B00 + slot], begin.wram[0x1B20 + slot]]))
            .collect();
        assert!(positions.contains(&x), "${x:04X} not in {positions:X?}");
        assert_eq!(end.wram[0x1D50 + child], 0, "child $7D50 cleared");
        assert_eq!(end.wram[0x1E90 + child], 0, "child $7E90 cleared");
    }
}
