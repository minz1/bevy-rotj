//! Type `$26`, the walker of stages 1-2 (`$02`) and 2-2 (`$06`) (bank 8 `$98D6`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work, anchor};
use crate::actor::families::Brain;
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, sound};
use crate::actors::CELL_PENDING;

/// Metasprite pointers indexed by `$7C30` (`$9C17`, drawn from bank 1).
const METASPRITES: u16 = 0x9C17;
/// The muzzle-flash metasprite (`$9C35`, bank 1).
const FLASH: u16 = 0x9C35;
/// The child `$9BB0` spawns into slots 16-23.
const CHILD: u8 = 0x24;

phases! {
    /// The walker's phase (`$7DD0`): the game stores its doubled index into `$9C27`.
    Phase {
        /// Idling, walking or hopping a gap.
        Walking = 0,
        /// Charging for 32 frames before it attacks.
        Charging = 2,
        /// Biting twice, spawning a `$24` child on each bite's 8th frame.
        Biting = 4,
    }
}

/// The walker's tables (bank 8).
pub struct Tables {
    /// The walk velocity, by the facing bit of `$99` (`$9C2D` low, `$9C2F` high).
    pub walk_speeds: [i16; 2],
    /// The bite's sprite, by `timer >> 2` (0-3) (`$9C31`).
    pub bite_sprites: [u8; 4],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            walk_speeds: engine::rom_split_words(prg, 8, 0x9C2F, 0x9C2D).map(u16::cast_signed),
            bite_sprites: engine::rom_bytes(prg, 8, 0x9C31),
        }
    }
}

/// Type `$26` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$98D6`; phases through the table at `$9C27`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        let mut probe = Probe::default();
        let mut flags = 0u8;
        if collision::feet(work, &mut probe, cx) {
            flags |= 0x40;
        }
        collision::head(work, &mut probe, cx);
        if collision::ahead(work, &mut probe, 0xC0, cx) {
            flags |= 0x80;
        }
        if work.vy >= 0 && flags & 0x40 == 0 {
            // The game passes rise = Y:A; Y is residue the port cannot see, but
            // `corners` only uses it while rising and here vy >= 0.
            if collision::corners(work, &mut probe, 0, 0x00C0, cx)
                || collision::ride_here(work, &mut probe, cx)
            {
                flags |= 0x20;
            }
        }
        if flags & 0x60 != 0 {
            body.vx_drift = drift(probe);
        }
        match Phase::from_byte(body.phase) {
            Some(Phase::Walking) => walking(body, work, &mut probe, flags, cx),
            Some(Phase::Charging) => charging(body, work, flags, cx),
            Some(Phase::Biting) => biting(body, cx),
            None => panic!("type $26 phase {} is past the $9C27 table", body.phase),
        }
        tail(kind, body, work, cx);
    }
}

/// The `$27/$28` landing bytes the game stores into `$7E50:$7E60`.
fn drift(probe: Probe) -> i16 {
    i16::from_be_bytes([probe.landing[1], probe.landing[0]])
}

/// The `$9C2D` walk velocity for the facing bit of `$99`.
fn walk(attr: u8, tables: &Tables) -> i16 {
    tables.walk_speeds[usize::from(attr & attr::FLIP_X != 0)]
}

/// Sets the facing bit of `$99` from the actor-minus-player x difference (`$9972`).
fn face(work: &mut Work, dx: i16) {
    work.attr = (work.attr & (attr::BEHIND | attr::PALETTE)) | engine::face(dx);
}

/// Walking (`$994E`): idle, walk, or hop a gap; charge when the player is within 6 px.
fn walking(body: &mut Body, work: &mut Work, probe: &mut Probe, flags: u8, cx: &mut Cx<'_>) {
    if body.timer != 0 {
        if flags & 0x60 != 0 {
            body.timer = 0;
            body.sprite = 0;
            body.vx = 0;
        } else {
            body.sprite = 6;
        }
        work.ax = 0;
        return;
    }
    let dx = engine::player_dx(work, &cx.effects.player);
    if dx.unsigned_abs().to_be_bytes()[0] < 6 {
        body.phase = Phase::Charging.byte();
        body.ref_vx = 0;
        body.anim = 0;
        return;
    }
    face(work, dx);
    if flags & 0x80 == 0 && collision::below(work, probe, cx) {
        body.ref_vx = walk(work.attr, &cx.prg.families.patrol_walker);
        if cx.world.frame.is_multiple_of(8) {
            body.sprite = body.sprite.wrapping_add(1);
        }
        if body.sprite >= 4 {
            body.sprite = 0;
        }
        return;
    }
    body.timer = 1;
    body.vx = walk(work.attr, &cx.prg.families.patrol_walker);
    body.ref_vx = 0;
    work.ax = 0;
    work.vy = -0x60;
}

/// Charging (`$9A13`): a 32-frame charge, then attack if the player is close enough.
fn charging(body: &mut Body, work: &mut Work, flags: u8, cx: &mut Cx<'_>) {
    if flags & 0x40 == 0 {
        body.vx = 0;
        work.ax = 0;
    }
    body.sprite = 7;
    body.anim.tick_up();
    if body.anim >= 0x20 {
        body.anim = 0x20;
        let dx = engine::player_dx(work, &cx.effects.player);
        face(work, dx);
        if dx.unsigned_abs().to_be_bytes()[0] >= 7 {
            body.phase = Phase::Walking.byte();
            body.timer = 0;
            return;
        }
        let dy = engine::player_dy(work, &cx.effects.player);
        if dy.unsigned_abs().to_be_bytes()[0] < 3 {
            body.phase = Phase::Biting.byte();
            body.timer = 0;
            body.anim = 0;
            body.ref_vx = 0;
            return;
        }
    }
    body.ref_vx = if !cx.world.frame.is_multiple_of(0x40) {
        0
    } else if cx.world.frame & 0x40 == 0 {
        0x20
    } else {
        -0x20
    };
}

/// Biting (`$9AC5`): two 16-frame bites, each spawning a `$24` child on its 8th frame.
fn biting(body: &mut Body, cx: &mut Cx<'_>) {
    body.counter = 0;
    body.timer.tick_up();
    if body.timer == 8 {
        cx.effects.sound[0] = sound::ENEMY_FIRES;
        spawn_child(body, cx);
    }
    if body.timer >= 0x10 {
        body.anim.tick_up();
        body.timer = 0;
        if body.anim >= 2 {
            body.phase = Phase::Walking.byte();
        }
        return;
    }
    let quarter = body.timer >> 2;
    body.sprite = cx.prg.families.patrol_walker.bite_sprites[usize::from(quarter)];
    if quarter == 2 {
        body.counter = 0x80;
    }
}

/// The shared tail from `$9B11`: draw, muzzle flash, contact and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    match engine::on_screen(work, cx.world) {
        Visibility::On => {}
        Visibility::Near => {
            work.age = work.age.wrapping_add(1);
            engine::end_frame(body, work);
            return;
        }
        Visibility::Far => {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
    }
    work.pointer = engine::rom_word(cx.prg, 8, METASPRITES + 2 * u16::from(body.sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, 1, cx);
    if body.counter & 0x80 != 0 {
        flash(work, cx);
    }
    if work.anchor & anchor::BOTTOM == 0 {
        work.hitbox = 0;
        engine::contact(kind, body, work, 2, cx);
    }
    work.age = work.age.wrapping_add(1);
    engine::end_frame(body, work);
}

/// The second draw `$9B4D` puts `$22` px above and `$1C` px to the side of the first.
fn flash(work: &mut Work, cx: &mut Cx<'_>) {
    let (x, y) = (work.screen_x, work.screen_y);
    let Some(up) = y.checked_sub(0x22) else {
        return;
    };
    work.screen_y = up;
    let side = if work.attr & attr::FLIP_X == 0 {
        x.checked_add(0x1C)
    } else {
        x.checked_sub(0x1C)
    };
    if let Some(side) = side {
        work.screen_x = side;
        work.pointer = engine::rom_word(cx.prg, 8, FLASH);
        work.attr |= attr::PALETTE;
        engine::draw(work, 1, cx);
    }
    work.screen_y = y;
    work.screen_x = x;
}

/// Writes the `$24` child into the highest free slot 16-23, as `$9BB0` does.
fn spawn_child(parent: &Body, cx: &mut Cx<'_>) {
    let Some(slot) = (16..24).rev().find(|&slot| cx.slots.main[slot].kind == 0) else {
        return;
    };
    let left = parent.flags & attr::FLIP_X != 0;
    let child = &mut cx.slots.main[slot];
    child.kind = CHILD;
    child.body.flags = parent.flags;
    child.body.phase = u8::from(left);
    child.body.x = if left {
        parent.x.wrapping_sub(0x1C0)
    } else {
        parent.x.wrapping_add(0x1C0)
    };
    child.body.y = parent.y.wrapping_sub(0x220);
    child.body.chr_slot = parent.chr_slot;
    child.body.state = 0;
    child.body.sprite = 2;
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The stage 1-2 walker: it walks in, charges, then bites twice with its `$24` children.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(2507) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 200, &[0x26]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 75,
                occupied: 43,
                drawn: 43,
                phases: BTreeMap::from([(0, 11), (2, 24), (4, 8)]),
            }
        );
    }

    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(2509) else {
            return;
        };
        scene.isolate(&[0x26]);
        let slot = {
            let wram = &scene.machine.bus.wram;
            (0..24).find(|&s| wram[0x1CC0 + s] == 0x26).expect("a $26")
        };
        let (x, y) = {
            let wram = &scene.machine.bus.wram;
            (
                u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]),
                u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]),
            )
        };
        scene.script(60, |_| 0);
        scene.player(x, y);
        // The wall next to the actor pushes the player out of reach; stand the actor
        // where the player settles so the two touch.
        scene.run(1, &[]);
        let ram = &scene.machine.bus.ram;
        let [hi, lo] = u16::from_be_bytes([ram[0x460], ram[0x461]]).to_be_bytes();
        let wram = &mut scene.machine.bus.wram;
        (wram[0x1B00 + slot], wram[0x1B20 + slot]) = (hi, lo);
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..60 {
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 60, &[0x26]);
        assert!(coverage.drawn > 0, "{coverage:?}");
    }

    #[test]
    fn hops_with_no_ground_below() {
        let Some(mut scene) = Scene::tas(2509) else {
            return;
        };
        scene.isolate(&[0x26]);
        let slot = {
            let wram = &scene.machine.bus.wram;
            (0..24).find(|&s| wram[0x1CC0 + s] == 0x26).expect("a $26")
        };
        scene.script(16, |_| 0);
        let [hi, lo] = 0x0200u16.to_be_bytes();
        let wram = &mut scene.machine.bus.wram;
        (wram[0x1B40 + slot], wram[0x1B60 + slot]) = (hi, lo);
        // With no ground below, walking takes the `$998C` hop, then `$99EC` until
        // the actor leaves the top of the screen and despawns.
        let coverage = assert_actor_phases(scene, 16, &[0x26]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 16,
                occupied: 9,
                drawn: 4,
                phases: BTreeMap::from([(0, 9)]),
            }
        );
    }
}
