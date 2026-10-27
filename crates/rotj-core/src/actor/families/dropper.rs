//! Type `$2E` (bank 11 `$85A7`): a dropper that lowers a `$30` child from the
//! top of the screen, then throws it at the player.
//!
//! The parent keeps the child's slot in `$7E90` and moves and draws the child
//! itself until the throw. The child's draw (`$8774`) swaps the child's
//! position into the working set and, when the child is off screen, never swaps
//! it back: the parent then draws and stores with the child's position.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::dropper_payload;
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Counter, contact};
use crate::actor::world::Cx;
use crate::actors::{CELL_PENDING, STATE_UNTARGETED};

const CHILD: u8 = 0x30;
const BANK: u8 = 11;
const BODY_SPRITES: u16 = 0x8852;
const CHILD_SPRITES: u16 = 0x885A;
const HITBOX: u8 = 0x0B;
const DAMAGE: u8 = 2;

/// The dropper's tables (bank 11).
pub struct Tables {
    /// The throw's low velocity byte when `dx < 0`, by reach 0-7 (`$886A`).
    pub throw_left: [u8; 8],
    /// The throw's low velocity byte when `dx >= 0`, by reach 0-7 (`$8872`).
    pub throw_right: [u8; 8],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            throw_left: engine::rom_bytes(prg, BANK, 0x886A),
            throw_right: engine::rom_bytes(prg, BANK, 0x8872),
        }
    }
}

phases! {
    /// The dropper's phase (`$7DD0`): the game stores its doubled index into `$8860`.
    Phase {
        /// Counting down before it lowers a child.
        Waiting = 0,
        /// Lowering the child, waiting for it to land; ends by throwing it.
        Lowering = 2,
        /// Waiting out the throw.
        Recovering = 4,
    }
}

/// Type `$2E` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$85A7`; phases through the table at `$8860`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            // The game frees the child on phase 1, which never occurs (0, 2, 4); kept for parity.
            if body.phase == 1 {
                free_child(body, cx);
            }
            return;
        }
        work.attr |= body.flags;
        match Phase::from_byte(body.phase) {
            Some(Phase::Waiting) => waiting(body, work, cx),
            Some(Phase::Lowering) => lowering(body, work, cx),
            Some(Phase::Recovering) => recovering(body),
            None => panic!("type $2E phase {} is past the $8860 table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// The child's slot, which the dropper keeps in `anim` (`$7E90`).
fn child_slot(body: &Body) -> usize {
    usize::from(body.anim)
}

fn free_child(body: &Body, cx: &mut Cx<'_>) {
    if let Some(child) = cx.slots.main.get_mut(child_slot(body)) {
        child.kind = 0;
    }
}

/// Waiting (`$85DA`): count down, then face the player and spawn the child.
fn waiting(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    body.timer.tick_down();
    if !body.timer.ran_out() {
        body.sprite = 0;
        return;
    }
    let dx = engine::player_dx(work, &cx.effects.player);
    work.attr = (work.attr & attr::PALETTE) | engine::face(dx);
    if spawn_child(body, work, cx) {
        body.phase = Phase::Lowering.byte();
        body.timer = 0;
        body.counter = 0;
    } else {
        body.timer = 0x40;
    }
}

/// `$86D1`: puts a `$30` in the highest free slot of 0-15; false when none is free.
fn spawn_child(body: &mut Body, work: &Work, cx: &mut Cx<'_>) -> bool {
    let Some(index) = cx.slots.free_main(0..16) else {
        return false;
    };
    body.anim = u8::try_from(index).expect("< 16");
    let camera_y = cx.world.camera_y;
    let child = cx.slots.main[index].spawn(CHILD, work.x, camera_y);
    child.chr_slot = body.chr_slot;
    child.vy = 0x40;
    child.contact = contact::PASSIVE;
    child.state = STATE_UNTARGETED;
    child.flags = (work.attr & attr::FLIP_X) | 0x02;
    child.phase = 0;
    true
}

/// Lowering (`$8616`): lower the child, then wait for it to land, then throw it.
fn lowering(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if body.timer == 0 {
        let (sprite, landed) = lower_child(body, work, cx);
        draw_child(body, work, sprite, cx);
        if landed {
            body.timer.tick_up();
        }
        body.sprite = 2;
        return;
    }
    body.counter.tick_up();
    if body.counter & 0x80 == 0 {
        if body.counter >= 4 {
            body.counter = 0x80;
        }
        draw_child(body, work, 0, cx);
        body.sprite = 1;
    } else if body.counter >= 0xAF {
        body.phase = Phase::Recovering.byte();
        body.timer = 0x40;
        throw_child(body, work, cx);
    } else {
        draw_child(body, work, 2, cx);
        body.sprite = 2;
    }
}

/// `$871F`: the child's fall; returns the sprite it takes and whether it reached the floor.
fn lower_child(body: &Body, work: &Work, cx: &mut Cx<'_>) -> (u8, bool) {
    let child = &mut cx.slots.main[child_slot(body)].body;
    child.y = child.y.wrapping_add_signed(child.vy);
    let next = child.vy.wrapping_add(4);
    if next.to_be_bytes()[1] & 0x80 == 0 {
        child.vy = next;
    }
    if child.y < work.y.wrapping_sub(0x03B0) {
        (4, false)
    } else {
        child.y = work.y;
        (0, true)
    }
}

/// `$8774`: draws the child through the working set.
fn draw_child(body: &Body, work: &mut Work, sprite: u8, cx: &mut Cx<'_>) {
    let saved = (work.x, work.y, work.chr_slot, work.attr);
    let child = &cx.slots.main[child_slot(body)].body;
    work.x = child.x;
    work.y = child.y;
    work.attr = child.flags;
    work.chr_slot = child.chr_slot;
    if engine::on_screen(work, cx.world) != Visibility::On {
        return;
    }
    work.pointer = engine::rom_word(cx.prg, BANK, CHILD_SPRITES + u16::from(sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, 1, cx);
    (work.x, work.y, work.chr_slot, work.attr) = saved;
}

/// `$87D3`: the child takes off toward the player.
fn throw_child(body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    let dx = engine::player_dx(work, &cx.effects.player);
    let reach = u16::from(dx.unsigned_abs().to_be_bytes()[0].min(7));
    let tables = &cx.prg.families.dropper;
    let (hi, lo) = if dx < 0 {
        (0x00, tables.throw_left[usize::from(reach)])
    } else {
        (0xFF, tables.throw_right[usize::from(reach)])
    };
    let child = &mut cx.slots.main[child_slot(body)].body;
    dropper_payload::arm(
        child,
        work.slot,
        cx.effects.player.y,
        work.y.wrapping_sub(0x03B0),
        i16::from_be_bytes([hi, lo]),
    );
}

/// Recovering (`$8678`): wait out the throw.
fn recovering(body: &mut Body) {
    body.sprite = 3;
    body.timer.tick_down();
    if body.timer.ran_out() {
        body.phase = Phase::Waiting.byte();
        body.timer = 0x40;
    }
}

/// `$868C`: draw, contact and end the frame; a far-off parent frees itself and its child.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {}
        Visibility::Near => return,
        Visibility::Far => {
            if body.phase != Phase::Waiting.byte() {
                free_child(body, cx);
            }
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
    }
    work.pointer = engine::rom_word(cx.prg, BANK, BODY_SPRITES + 2 * u16::from(body.sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, 1, cx);
    work.hitbox = HITBOX;
    engine::contact(kind, body, work, DAMAGE, cx);
    engine::end_frame(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    const FRAME: u64 = 13613;

    fn poke(scene: &mut Scene, addr: u16, value: u8) {
        scene.machine.bus.trace.pokes.push((0xD450, addr, value));
    }

    fn free_children(scene: &mut Scene) {
        for slot in 3..16 {
            poke(scene, 0x7CC0 + slot, 0);
        }
    }

    fn scene_at(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        free_children(&mut scene);
        Some(scene)
    }

    fn parents(scene: &Scene) -> Vec<usize> {
        (0..16)
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x2E)
            .collect()
    }

    fn phase(scene: &Scene, slot: usize) -> u8 {
        scene.machine.bus.wram[0x1DD0 + slot]
    }

    /// Runs until a parent is in `phase` and returns its slot.
    fn until_phase(scene: &mut Scene, wanted: u8) -> usize {
        for _ in 0..400 {
            scene.run(1, &[]);
            if let Some(slot) = parents(scene)
                .into_iter()
                .find(|&slot| phase(scene, slot) == wanted)
            {
                return slot;
            }
        }
        panic!("no $2E reached phase {wanted}");
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene_at(FRAME) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 399,
                occupied: 399,
                drawn: 399,
                phases: BTreeMap::from([(0, 130), (2, 139), (4, 130)]),
            }
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = scene_at(FRAME) else {
            return;
        };
        scene.isolate(&[0x2E]);
        let slot = parents(&scene)[0];
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.script(200, |_| 0);
        scene.player(x, y);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..40 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 40, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 40,
                drawn: 40,
                phases: BTreeMap::from([(2, 40)]),
            }
        );
    }

    #[test]
    fn dying_while_the_child_is_out() {
        let Some(mut scene) = scene_at(FRAME) else {
            return;
        };
        scene.isolate(&[0x2E]);
        let slot = until_phase(&mut scene, 2);
        poke(&mut scene, 0x7D70 + u16::try_from(slot).unwrap(), 0x80);
        let coverage = assert_actor_phases(scene, 40, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 40,
                occupied: 16,
                drawn: 17,
                phases: BTreeMap::from([(2, 16)]),
            }
        );
    }

    #[test]
    fn dying_in_phase_one_frees_the_child_slot() {
        let Some(mut scene) = scene_at(FRAME) else {
            return;
        };
        scene.isolate(&[0x2E]);
        let slot = until_phase(&mut scene, 2);
        let slot = u16::try_from(slot).unwrap();
        let child = scene.machine.bus.wram[0x1E90 + usize::from(slot)];
        scene.machine.bus.wram[0x1CC0 + usize::from(child)] = 0x30;
        poke(&mut scene, 0x7D70 + slot, 0x80);
        poke(&mut scene, 0x7DD0 + slot, 1);
        let coverage = assert_actor_phases(scene, 4, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 4,
                occupied: 4,
                drawn: 4,
                phases: BTreeMap::from([(1, 4)]),
            }
        );
    }

    fn far_scene(wanted: u8, offset: u16) -> Option<Scene> {
        let mut scene = scene_at(FRAME)?;
        scene.isolate(&[0x2E]);
        let slot = u16::try_from(until_phase(&mut scene, wanted)).unwrap();
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(offset).to_be_bytes();
        poke(&mut scene, 0x7B00 + slot, hi);
        poke(&mut scene, 0x7B20 + slot, lo);
        Some(scene)
    }

    #[test]
    fn far_away_frees_the_slot_and_the_child() {
        let Some(scene) = far_scene(2, 500 * 16) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 6, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 6,
                occupied: 1,
                drawn: 1,
                phases: BTreeMap::from([(2, 1)]),
            }
        );
    }

    #[test]
    fn far_away_in_phase_zero_frees_only_the_slot() {
        let Some(scene) = far_scene(0, 500 * 16) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 6, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 6,
                occupied: 1,
                drawn: 0,
                phases: BTreeMap::from([(0, 1)]),
            }
        );
    }

    #[test]
    fn near_the_edge_nothing_runs() {
        let Some(scene) = far_scene(2, 300 * 16) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 6, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 6,
                occupied: 6,
                drawn: 6,
                phases: BTreeMap::from([(2, 6)]),
            }
        );
    }

    #[test]
    fn matches_the_oracle_when_the_weapon_kills_it() {
        let Some(scene) = scene_at(20000) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 400, &[0x2E]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 282,
                occupied: 63,
                drawn: 169,
                phases: BTreeMap::from([(0, 1), (2, 62)]),
            }
        );
    }

    /// The child's `vy` gain, kept while its low byte stays under `$80`, is the byte
    /// pair it replaced.
    #[test]
    fn the_child_gain_is_word_maths() {
        for vy in i16::MIN..=i16::MAX {
            let [hi, lo] = vy.to_be_bytes();
            let (next, carry) = lo.overflowing_add(4);
            let bytes = if next & 0x80 == 0 {
                i16::from_be_bytes([hi.wrapping_add(u8::from(carry)), next])
            } else {
                vy
            };
            let word = vy.wrapping_add(4);
            let kept = if word.to_be_bytes()[1] & 0x80 == 0 {
                word
            } else {
                vy
            };
            assert_eq!(kept, bytes, "{vy}");
        }
    }
}
