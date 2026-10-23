//! Type `$1E` (bank 8 `$9364`): hovers in place, faces the player, then rears a
//! `$20` companion out to its side.

use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Counter};
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// The frame-indexed metasprite pointer table (bank 8 `$94A0`), word-indexed by `$7E10`.
const METASPRITES: u16 = 0x94A0;
/// The companion slot type the `$9457` routine rears.
const CHILD: u8 = 0x20;
/// `$937F`'s gate: the sprite only rolls every fourth frame.
const ROLL_PERIOD: u8 = 4;
/// `$7DD0` while it hovers; any other value rears a child.
const HOVERING: u8 = 0;
/// `$7DD0` while it rears a child.
const REARING: u8 = 1;

/// Type `$1E` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9364`): hovering, then rearing a child.
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr = (body.flags | work.attr) & !attr::BEHIND;
        if body.phase == HOVERING {
            hovering(body, work, cx);
        } else {
            rearing(body, work, cx);
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
        work.pointer = engine::rom_word(cx.prg, 8, METASPRITES + 2 * u16::from(body.counter));
        engine::tile_bits(work, cx.prg);
        engine::draw(work, 1, cx);
        if work.anchor & anchor::BOTTOM == 0 {
            work.hitbox = 0x0A;
            engine::contact(kind, body, work, 2, cx);
        }
        engine::end_frame(body, work);
    }
}

/// Hovering (`$937F`): the RNG picks a sprite until the player comes level, then
/// the actor rears a child.
fn hovering(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if cx.world.frame.is_multiple_of(ROLL_PERIOD) {
        body.counter = if cx.world.rng[0] >= 0xE8 { 8 } else { 0 };
    }
    let dx = engine::player_dx(work, &cx.effects.player);
    if dx.unsigned_abs() >= 0x200 {
        face(work, dx);
    }
    if body.timer != 0 {
        body.timer -= 1;
    } else {
        let dy = engine::player_dy(work, &cx.effects.player);
        if (dy.unsigned_abs() >> 8) as u8 & 0x0F < 3 {
            body.phase = REARING;
            body.anim = 0;
        }
    }
}

/// Rearing (`$93D0`): count to `$20` in `$7E90`, re-arming a `$20` child at 4.
fn rearing(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    if body.anim == 0 {
        face(work, engine::player_dx(work, &cx.effects.player));
    }
    body.anim.tick_up();
    if body.anim == 4
        && let Some(slot) = engine::free_child_slot(cx.slots)
    {
        spawn_child(body, slot, cx);
    }
    if body.anim == 0x20 {
        body.phase = HOVERING;
        body.counter = 0;
        body.timer = 0x80;
    } else {
        body.counter = body.anim >> 2;
    }
}

/// Turns to face the player `dx` away (`$939C`; hovering only from `$200` away, rearing's
/// `$93D8` at any distance).
fn face(work: &mut Work, dx: i16) {
    work.attr = (work.attr & attr::PALETTE) | engine::face(dx);
}

/// Writes the `$20` companion into slot `slot` (16-23) as `$9457` does: `$200`
/// to the side the parent faces, `$17F` above it, sharing its flags and CHR slot.
fn spawn_child(parent: &Body, slot: usize, cx: &mut Cx<'_>) {
    let facing_left = parent.flags & attr::FLIP_X != 0;
    let x = if facing_left {
        parent.x.wrapping_sub(0x200)
    } else {
        parent.x.wrapping_add(0x200)
    };
    let child = cx.slots.main[slot].spawn(CHILD, x, parent.y.wrapping_sub(0x17F));
    child.flags = parent.flags;
    child.chr_slot = parent.chr_slot;
    child.state = 0;
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// Runs `$1E` from `frame` of the TAS. The `$20` children it rears are cleared
    /// each loop, so their handler never runs.
    fn tas_at(frame: u64) -> Option<Scene> {
        let mut scene = Scene::tas(frame)?;
        for slot in 16..24 {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, 0x7CC0 + slot, 0));
        }
        Some(scene)
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = tas_at(3830) else {
            return;
        };
        // The window runs past the hit actor's death into the `$10` item it
        // becomes (`$A56E`'s tail), so the diff follows both handlers. Area 2's
        // FME-7 IRQ fires inside the actor loop, so `brackets` drops those frames.
        let coverage = assert_actor_phases(scene, 378, &[0x1E]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (242, 110, 225),
            "{coverage:?}"
        );
        assert_eq!(
            coverage.phases,
            std::collections::BTreeMap::from([(0, 81), (1, 29)]),
            "{coverage:?}"
        );
    }

    /// The same kind from its spawn in stage 1-1 room 2 (`docs/ACTORS.md` §8b),
    /// covering phase 0 before the player is level.
    #[test]
    fn matches_the_oracle_from_spawn() {
        let Some(scene) = tas_at(781) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 20, &[0x1E]);
        assert_eq!(
            (coverage.occupied, coverage.drawn),
            (19, 19),
            "{coverage:?}"
        );
        assert_eq!(coverage.phases.get(&0), Some(&19), "{coverage:?}");
    }
}
