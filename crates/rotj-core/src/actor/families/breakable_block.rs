//! Types `$04` and `$06` (bank 8 `$823C`): a breakable block that never integrates
//! velocity, flashes when hit, shatters into the level when killed and despawns when
//! far off screen.

use crate::actor::engine::{self, TileBlock, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Both kinds keep no state outside [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$823C`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        work.attr = body.flags;
        engine::load(body, work);
        match engine::on_screen(work, cx.world) {
            Visibility::Far => {
                engine::despawn(kind, body, 0x80, cx.schedule);
                return;
            }
            Visibility::Near if work.anchor == 0 => {}
            _ => {
                if body.hit & 0x80 != 0 {
                    if body.dying_timer == 0 {
                        if cx.effects.vram.top < 0x60 {
                            return;
                        }
                        cx.effects.sound[1] = 0x20;
                        _ = engine::spawn_parts(0, work, cx);
                        _ = engine::spawn_parts(1, work, cx);
                        shatter(body, work, cx);
                    }
                    engine::dying(kind, body, work, 0, cx);
                    return;
                }
                if body.hit & 0x40 != 0 {
                    work.pointer = engine::rom_word(cx.prg, 8, 0x84DE);
                    work.chr_slot = 0;
                    work.attr = 3;
                    engine::draw(work, 8, cx);
                    body.hit = 0;
                }
                if work.anchor == 0 {
                    work.hitbox = 1;
                    engine::contact(kind, body, work, 1, cx);
                }
            }
        }
        work.age = work.age.wrapping_add(1);
        engine::end_frame(body, work);
    }
}

fn shatter(body: &Body, work: &Work, cx: &mut Cx<'_>) {
    let column = work.x.to_be_bytes()[0];
    let row = work.y.wrapping_sub(0x05D0).to_be_bytes()[0];
    let above = Work {
        y: work.y.wrapping_sub(0x04C0),
        ..*work
    };
    let (base, _) = engine::map_page(&above, cx);
    let target = u16::from_be_bytes([0x60 | (row & 0x0F), base | (column & 0x0F)]);
    let source = u16::from_be_bytes([body.subtype, body.behaviour]);
    let cells = [0x000, 0x001, 0x100, 0x101];
    let level = &mut cx.effects.level;
    let mut tiles = [0u8; 16];
    for (i, cell) in cells.into_iter().enumerate() {
        let id = level.map[usize::from(source.wrapping_add(cell) - 0x6000)];
        level.map[usize::from(target.wrapping_add(cell) - 0x6000)] = id;
        let [top_left, top_right, bottom_left, bottom_right] = level.tiles(id);
        let at = (i / 2) * 8 + (i % 2) * 2;
        tiles[at..at + 2].copy_from_slice(&[top_left, top_right]);
        tiles[at + 4..at + 6].copy_from_slice(&[bottom_left, bottom_right]);
    }
    let fields = (body.flags & 0x80 == 0).then(|| {
        let [target_hi, target_lo] = target.to_be_bytes();
        let hi = if engine::is_page_row(base, cx) {
            target_hi.wrapping_sub(1)
        } else {
            target_hi
        };
        copy_palettes(
            source,
            (u16::from_be_bytes([hi, target_lo]), target_hi),
            &mut cx.effects.level.attributes,
        )
    });
    let block = TileBlock {
        column,
        row,
        width: 4,
        height: 4,
        tiles: &tiles,
        fields: fields.as_ref().map(<[u8; 4]>::as_slice),
    };
    engine::queue_block(&block, cx);
}

fn copy_palettes(
    source: u16,
    (target, target_hi): (u16, u8),
    attributes: &mut [u8; 1024],
) -> [u8; 4] {
    let quad = |addr: u16| {
        let [hi, lo] = addr.to_be_bytes();
        usize::from(lo >> 4) * 64 + usize::from(hi & 0x0E) * 4 + usize::from(lo & 0x0E) / 2
    };
    let field_of = |addr: u16| usize::from((addr & 1) | ((addr >> 7) & 2));
    let next_row = |addr: u16, hi: u8, base: usize| {
        let addr = u16::from_be_bytes([hi, addr.to_be_bytes()[1]]);
        (addr, base + if hi & 1 == 0 { 8 } else { 0 })
    };
    let (source_base, target_base) = (quad(source), quad(target));
    let (mut from, mut from_at) = (source, source_base);
    let (mut to, mut to_at) = (target, target_base);
    let mut fields = [0u8; 4];
    for (row, pair) in fields.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        if row == 1 {
            (from, from_at) =
                next_row(source, source.to_be_bytes()[0].wrapping_add(1), source_base);
            (to, to_at) = next_row(target, target_hi.wrapping_add(1), target_base);
        }
        for field in pair {
            let slot = field_of(from);
            *field = (attributes[from_at] >> (2 * slot)) & 3;
            let slot = field_of(to);
            attributes[to_at] =
                (attributes[to_at] & engine::FIELD_MASKS[slot]) | (*field << (2 * slot));
            from = from.wrapping_add(1);
            if from & 1 == 0 {
                from_at += 1;
            }
            to = to.wrapping_add(1);
            if to & 1 == 0 {
                to_at += 1;
            }
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases, assert_routine};

    fn slot_of(scene: &Scene) -> usize {
        (0..16)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x06)
            .expect("a live $06")
    }

    /// Route frames 701-800: the room-spawned `$06` sits on screen, in phase 0.
    /// Its `$7EE0` has bit 6 clear, so it never draws and never checks the
    /// player; only the weapon scan of `$A9C4` runs.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::route(700) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 100, &[0x06]);
        let expected = Coverage {
            frames: 100,
            occupied: 100,
            drawn: 0,
            phases: BTreeMap::from([(0, 100)]),
        };
        assert_eq!(coverage, expected);
    }

    /// The `$7D70` bit-6 flash (bank 8 `$8FC4`) draws for as long as the hit
    /// stays set; poking it each frame is the only way to reach `$8266`.
    #[test]
    fn matches_the_oracle_when_hit() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        let hit = 0x7D70 + u16::try_from(slot_of(&scene)).expect("slot fits");
        scene.machine.bus.trace.pokes.push((0xD450, hit, 0x42));
        let coverage = assert_actor_phases(scene, 6, &[0x06]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (6, 6, 6)
        );
    }

    /// `$8252`: an actor past the far margin frees its slot and re-arms its cell.
    #[test]
    fn matches_the_oracle_off_screen() {
        let Some(mut scene) = Scene::route(700) else {
            return;
        };
        let slot = slot_of(&scene);
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let [hi, lo] = camera.wrapping_add(400 * 16).to_be_bytes();
        (
            scene.machine.bus.wram[0x1B00 + slot],
            scene.machine.bus.wram[0x1B20 + slot],
        ) = (hi, lo);
        let coverage = assert_actor_phases(scene, 5, &[0x06]);
        assert_eq!((coverage.frames, coverage.occupied), (5, 1));
    }

    /// TAS frames where a block shatters (bank 8 `$82A6`): horizontal levels in areas
    /// 0, 2 and 6, vertical ones (`$DD = $40`, `$C0`), and a wait for queue room.
    #[test]
    fn matches_the_oracle_when_shattered() {
        for (frame, frames) in [
            (429, 1),
            (488, 1),
            (813, 1),
            (1031, 1),
            (1230, 1),
            (7470, 1),
            (7514, 6),
            (7812, 1),
        ] {
            let Some(scene) = Scene::tas(frame - 1) else {
                return;
            };
            let coverage = assert_actor_phases(scene, frames, &[0x04, 0x06]);
            assert_eq!(
                coverage.frames,
                usize::try_from(frames).expect("small"),
                "frame {frame}"
            );
        }
    }

    /// Area 2's IRQ splits its actor loops, so its shatters are checked as the call itself.
    #[test]
    fn shatter_matches_82bf_in_area_2() {
        let mut checked = 0;
        for frame in [2869, 3424, 3564, 5344, 5568] {
            let Some(mut scene) = Scene::tas(frame - 1) else {
                return;
            };
            checked += assert_routine(&mut scene, 1, (0x82BF, 8), &[0x82B7], |call| {
                call.in_slot(|slot, work, cx| super::shatter(&slot.body, work, cx));
            });
        }
        assert_eq!(checked, 2);
    }
}
