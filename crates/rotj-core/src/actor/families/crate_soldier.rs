//! Type `$22`, the stage 1-1 soldier on the crate (bank 8 `$9555`).

use crate::actor::collision::{self, Probe};
use crate::actor::engine::{self, Visibility, Work};
use crate::actor::families::Brain;
use crate::actor::oam::attr;
use crate::actor::slots::Body;
use crate::actor::world::Cx;
use crate::actors::CELL_PENDING;

/// Metasprites indexed by `$7C30` (bank 8 `$9813`, byte-indexed).
const METASPRITES: u16 = 0x9813;
/// The muzzle-flash metasprite (bank 8 `$981D`).
const SHOT: u16 = 0x981D;
/// The child spawned at `$97FF`.
const CHILD: u8 = 0x24;

phases! {
    /// The soldier's phase (`$7DD0`), which picks its handler (`$980B`).
    Phase {
        /// Falling until the feet find ground.
        Falling = 0,
        /// Idling on the timer, then jumping or shooting.
        Idling = 1,
        /// Walking in, then firing up to four `$24` children.
        Firing = 2,
        /// The recoil jump, until it lands.
        Recoiling = 3,
    }
}

/// The soldier's tables (bank 8).
pub struct Tables {
    /// The recoil jump's x velocity, by its facing bit (`$981F` low, `$9821` high).
    pub turn_speeds: [i16; 2],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            turn_speeds: engine::rom_split_words(prg, 8, 0x9821, 0x981F).map(u16::cast_signed),
        }
    }
}

/// Type `$22` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9555`; phases through the table at `$980B`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, 0x80, cx) {
            return;
        }
        work.attr |= body.flags;
        engine::integrate(body, work);
        let mut probe = Probe::default();
        match Phase::from_byte(body.phase) {
            Some(Phase::Falling) => falling(body, work, &mut probe, cx),
            Some(Phase::Idling) => idling(body, work, &mut probe, cx),
            Some(Phase::Firing) => firing(body, work, &mut probe, cx),
            Some(Phase::Recoiling) => recoiling(body, work, &mut probe, cx),
            None => panic!("type $22 phase {} is past the $980B table", body.phase),
        }
        tail(kind, body, work, &mut probe, cx);
    }
}

/// `$A7DD` then `$AF6A`: whether the feet probe or a platform landed the actor.
fn on_ground(work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) -> bool {
    collision::feet(work, probe, cx) || collision::ride_here(work, probe, cx)
}

/// Falling (`$957E`): fall until the feet find ground, then idle.
fn falling(body: &mut Body, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    if on_ground(work, probe, cx) {
        body.vx_drift = drift(*probe);
        body.timer = 0x20;
        body.phase = Phase::Idling.byte();
        body.anim = 1;
        body.sprite = 2;
    } else {
        body.sprite = 0;
    }
}

/// Idling (`$95B0`): idle on the timer, then face the player and jump or shoot.
fn idling(body: &mut Body, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    if on_ground(work, probe, cx) {
        body.vx_drift = drift(*probe);
    }
    body.timer = body.timer.wrapping_sub(1);
    if body.timer != 0 {
        anim(body);
        return;
    }
    let dx = engine::player_dx(work, &cx.effects.player);
    let dx_hi = dx.unsigned_abs().to_be_bytes()[0];
    work.attr = (work.attr & attr::PALETTE) | engine::face(dx);
    if dx_hi >= 5 {
        body.phase = Phase::Firing.byte();
        body.timer = 0;
        body.counter = 0;
        anim(body);
    } else {
        let turn = if dx_hi >= 3 {
            u8::from(work.attr & attr::FLIP_X != 0)
        } else {
            u8::from(work.attr & attr::FLIP_X == 0)
        };
        body.vx = cx.prg.families.crate_soldier.turn_speeds[usize::from(turn)];
        work.vy = -0x60;
        body.phase = Phase::Recoiling.byte();
    }
}

/// Firing (`$963C`): walk in, then fire up to four `$24` children.
fn firing(body: &mut Body, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    if on_ground(work, probe, cx) {
        body.vx_drift = drift(*probe);
    }
    body.timer = body.timer.wrapping_add(1);
    if body.timer < 0x18 {
        body.sprite = 4;
        return;
    }
    if body.timer & 0x07 != 0 {
        body.sprite = 6;
        return;
    }
    body.counter = body.counter.wrapping_add(1);
    if body.counter >= 4 {
        body.phase = Phase::Idling.byte();
        body.anim = 1;
        body.timer = 0x80;
        body.sprite = 2;
    } else {
        if let Some(slot) = engine::free_child_slot(cx.slots) {
            cx.effects.sound[0] = 0x28;
            spawn_child(body, slot, cx);
        }
        body.sprite = 6;
    }
}

/// Recoiling (`$969E`): the recoil jump; land and go back to the idle timer.
fn recoiling(body: &mut Body, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    let mut flags = 0u8;
    work.ax = 0;
    if collision::feet(work, probe, cx) {
        flags |= 0x40;
    } else if work.vy >= 0 {
        // The game passes rise = Y:A; Y is residue the port cannot see, but
        // `corners` only uses it while rising and here vy >= 0.
        if collision::corners(work, probe, 0, 0x0070, cx) {
            flags |= 0x40;
        }
    }
    if flags & 0x40 == 0 && collision::ride_here(work, probe, cx) {
        flags |= 0x40;
    }
    collision::head(work, probe, cx);
    collision::ahead(work, probe, 0xA0, cx);
    if flags & 0x40 != 0 {
        body.vx_drift = drift(*probe);
        body.vx = 0;
        body.phase = Phase::Idling.byte();
        body.anim = 1;
        body.timer = 0x20;
        body.sprite = 2;
    } else {
        body.sprite = 0;
    }
}

/// The shared tail from `$970A`: draw, muzzle flash, contact and store.
fn tail(kind: &mut u8, body: &mut Body, work: &mut Work, probe: &mut Probe, cx: &mut Cx<'_>) {
    collision::head(work, probe, cx);
    match engine::on_screen(work, cx.world) {
        Visibility::On => {}
        Visibility::Near => {
            engine::end_frame(body, work);
            return;
        }
        Visibility::Far => {
            engine::despawn(kind, body, CELL_PENDING, cx.schedule);
            return;
        }
    }
    work.pointer = engine::rom_word(cx.prg, 8, METASPRITES + u16::from(body.sprite));
    engine::tile_bits(work, cx.prg);
    engine::draw(work, 1, cx);
    if body.phase == Phase::Firing.byte()
        && body.timer >= 0x18
        && body.timer & 0x06 == 0
        && let Some(x) = shot_x(work)
    {
        work.screen_x = x;
        if let Some(y) = work.screen_y.checked_sub(0x18) {
            work.screen_y = y;
            work.pointer = engine::rom_word(cx.prg, 8, SHOT);
            work.attr = (work.attr & (attr::FLIP_X | attr::FLIP_Y))
                | if cx.world.frame_odd { attr::PALETTE } else { 0 };
            engine::draw(work, 1, cx);
        }
    }
    if work.anchor & 0x40 == 0 {
        work.hitbox = 0x0A;
        engine::contact(kind, body, work, 2, cx);
    }
    engine::end_frame(body, work);
}

/// The `$27/$28` landing bytes the game stores into `$7E50:$7E60`.
fn drift(probe: Probe) -> i16 {
    i16::from_be_bytes([probe.landing[1], probe.landing[0]])
}

/// The `$9622` walk exists: `$7C30` selects a metasprite and `$7E90` counts.
fn anim(body: &mut Body) {
    let mut frame = 8u8;
    if body.anim >= 0x0C {
        body.anim = 0x80;
        frame = 2;
    }
    body.sprite = frame;
    body.anim = body.anim.wrapping_add(1);
}

/// The muzzle-flash screen x at `$9747`; `None` skips the second draw as on a borrow.
fn shot_x(work: &Work) -> Option<u8> {
    if work.attr & attr::FLIP_X == 0 {
        work.screen_x
            .checked_add(0x18)
            .or_else(|| work.screen_x.checked_sub(0x18))
    } else {
        work.screen_x.checked_sub(0x18)
    }
}

/// Writes the `$24` child into main slot `slot` (16-23) as `$97B1` does.
fn spawn_child(parent: &Body, slot: usize, cx: &mut Cx<'_>) {
    let child = &mut cx.slots.main[slot];
    let facing_left = parent.flags & attr::FLIP_X != 0;
    child.kind = CHILD;
    child.body.flags = parent.flags;
    child.body.x = if facing_left {
        parent.x.wrapping_sub(0x180)
    } else {
        parent.x.wrapping_add(0x180)
    };
    child.body.y = parent.y.wrapping_sub(0x17F);
    child.body.chr_slot = parent.chr_slot;
    child.body.phase = u8::from(facing_left);
    child.body.state = 0;
    child.body.sprite = 0;
    child.brain = Brain::new(CHILD);
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The route's soldier: falls, idles, jumps at the player, shoots, and spawns
    /// its `$24` shots, which the poke removes each loop.
    fn route_at(frame: u64) -> Option<Scene> {
        let mut scene = Scene::route(frame)?;
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
    fn hurts_the_player_up_close() {
        let Some(mut scene) = route_at(900) else {
            return;
        };
        scene.isolate(&[0x22]);
        let wram = &scene.machine.bus.wram;
        let x = u16::from_be_bytes([wram[0x1B00], wram[0x1B20]]);
        let y = u16::from_be_bytes([wram[0x1B40], wram[0x1B60]]);
        scene.player(x.wrapping_sub(0x100), y);
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
        let coverage = assert_actor_phases(scene, 120, &[0x22]);
        assert_eq!(
            (coverage.frames, coverage.occupied),
            (120, 120),
            "{coverage:?}"
        );
    }

    #[test]
    fn off_screen_despawns() {
        let Some(mut scene) = route_at(900) else {
            return;
        };
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let wram = &mut scene.machine.bus.wram;
        let [hi, lo] = (camera + 400 * 16).to_be_bytes();
        (wram[0x1B00], wram[0x1B20]) = (hi, lo);
        let coverage = assert_actor_phases(scene, 30, &[0x22]);
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
        assert!(coverage.occupied < 30, "{coverage:?}");
    }

    #[test]
    fn near_off_screen_is_stored() {
        let Some(mut scene) = route_at(900) else {
            return;
        };
        let camera = u16::from_be_bytes([scene.machine.bus.ram[0xCC], scene.machine.bus.ram[0xCD]]);
        let wram = &mut scene.machine.bus.wram;
        let [hi, lo] = (camera + 300 * 16).to_be_bytes();
        (wram[0x1B00], wram[0x1B20]) = (hi, lo);
        let coverage = assert_actor_phases(scene, 30, &[0x22]);
        assert_eq!(
            (coverage.frames, coverage.occupied),
            (30, 30),
            "{coverage:?}"
        );
        assert_eq!(coverage.drawn, 0, "{coverage:?}");
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = route_at(845) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x22]);
        assert_eq!((coverage.frames, coverage.occupied), (300, 300));
        assert!(coverage.drawn > 0, "{coverage:?}");
        for phase in 0..4 {
            assert!(
                coverage.phases.get(&phase).is_some_and(|&n| n > 0),
                "phase {phase} unreached: {:?}",
                coverage.phases
            );
        }
    }
}
