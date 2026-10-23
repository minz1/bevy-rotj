//! Type `$42` (bank 11 `$947A`): a boss that charges a counter on every other
//! frame, drifts sideways when hit, and draws a six-cell energy bar.
//!
//! `$7DD0` is the phase: waiting, drifting (decaying the sideways drift) or
//! charged (counter at `$C0`, it only draws). It calls only routines in
//! `docs/ACTORS.md` §9c: `$A000`
//! (`load`), `$AF97` (`boss_flash`), `$B1C4` (`boss_defeated`), `$A79B`
//! (`integrate`), `$A852` (`on_screen`), `$C247` (`draw`, bank 9 as `$C229`
//! maps it), `$AFD6` (`boss_contact`), `$AF38` (`free_child_slot`) and `$A934`
//! (`store`).

use crate::actor::engine::{self, Work};
use crate::actor::slots::hit;
use crate::actor::slots::{Body, Counter};
use crate::actor::world::{Cx, Motion};

const KIND: u8 = 0x42;
const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// The body metasprite (`$96F9`), then one per `$7DF0` value (`$9713`).
const BODY_SPRITE: u16 = 0x96F9;
const BODY_SPRITE_BY_TIMER: u16 = 0x9713;
/// The bar's frame (`$96FB`) and its cells by fill (`$9727`).
const BAR_FRAME: u16 = 0x96FB;
const BAR_CELLS: u16 = 0x9727;
const SHOT_KIND: u8 = 0x44;
const CHILD_KIND: u8 = 0x46;
const CHILD_SLOT: usize = 23;
const FRAME_X: u8 = 0x80;
const FRAME_Y: u8 = 0x20;
const FIRST_CELL_X: u8 = 0x6C;
const LAST_CELL: u8 = 5;
const CELL_SPACING: u8 = 8;
const FULL_CELL: u8 = 0x10;
const BAR_Y: u8 = 0x4F;
const CHARGED: u8 = 0xC0;
const CONTACT: u16 = 0x00E0;
const KNOCK: i16 = 0x40;

phases! {
    /// The boss's phase (`$7DD0`), which picks its handler (`$96FD`).
    Phase {
        /// Stepping its animation and charging the counter.
        Waiting = 0,
        /// Drifting sideways after a hit, charging the counter.
        Drifting = 1,
        /// The counter reached `$C0`: only draws.
        Charged = 2,
    }
}

/// Type `$42`'s tables (bank 11).
pub struct Tables {
    /// The timer for this animation step, by the step `(anim & $7F) >> 3` (0-15) (`$9703`).
    pub step_timers: [u8; 16],
    /// A `$44` shot's x, low/high by `rng & 7` (`$9717`/`$971F`).
    pub shot_x: [u16; 8],
}

impl Tables {
    /// Reads the tables from the PRG.
    pub(crate) fn import(prg: &[u8]) -> Tables {
        Tables {
            step_timers: engine::rom_bytes(prg, HOME_BANK, 0x9703),
            shot_x: engine::rom_split_words(prg, HOME_BANK, 0x971F, 0x9717),
        }
    }
}

/// Type `$42` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$947A`).
    pub fn step(&mut self, _kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let flash = engine::boss_flash(body, work, cx.world);
        if flash & 0x80 != 0 {
            work.attr = 0;
            if body.dying_timer == 0x01 {
                engine::boss_defeated(own_slot(cx), cx);
            }
            if flash & 0x40 != 0 {
                finish(body, work, cx);
            } else {
                draw_bar(body, work, cx);
            }
            return;
        }
        work.attr |= body.flags;
        body.vy = 0;
        engine::integrate(body, work);
        match Phase::from_byte(body.phase) {
            Some(Phase::Waiting) => {
                wait(body, cx);
                charge(body, cx);
            }
            Some(Phase::Drifting) => {
                decay_drift(body, work);
                charge(body, cx);
            }
            Some(Phase::Charged) => {}
            None => panic!("type $42 phase {} is past the $96FD table", body.phase),
        }
        finish(body, work, cx);
    }
}

/// The slot the running `$42` actor occupies (`$10`).
fn own_slot(cx: &Cx<'_>) -> u8 {
    let index = cx
        .slots
        .main
        .iter()
        .position(|slot| slot.kind == KIND)
        .expect("the $42 slot");
    u8::try_from(index).expect("slot < 24")
}

/// `$94D0`: step the animation and take this step's timer.
fn wait(body: &mut Body, cx: &mut Cx<'_>) {
    body.anim.tick_up();
    let step = usize::from((body.anim & 0x7F) >> 3);
    body.timer = cx.prg.families.stage4_boss.step_timers[step];
}

/// `$94F9`/`$94FD`: the drift is spent, back to waiting.
fn settle(body: &mut Body) {
    body.vx_drift = 0;
    body.phase = Phase::Waiting.byte();
}

/// `$94E5`: pull the drift back to zero by 4 a frame, snapping at the edges.
fn decay_drift(body: &mut Body, work: &mut Work) {
    body.timer = 0;
    let x_high = work.x.to_be_bytes()[0];
    if body.vx_drift >= 0 {
        if x_high >= 0x0A {
            work.x = 0x0A00;
            settle(body);
            return;
        }
        body.vx_drift = body.vx_drift.wrapping_sub(4);
        let [high, low] = body.vx_drift.to_be_bytes();
        if high == 0 && low < 0x0B {
            work.x &= 0xFF00;
            settle(body);
        }
    } else {
        if x_high < 0x05 {
            work.x &= 0xFF00;
            settle(body);
            return;
        }
        body.vx_drift = body.vx_drift.wrapping_add(4);
        let [high, low] = body.vx_drift.to_be_bytes();
        if high != 0xFF {
            work.x &= 0xFF00;
            settle(body);
        } else if low >= 0xF4 {
            settle(body);
        }
    }
}

/// `$954A`: on even frames shoot, on odd frames charge; at `$C0` become charged.
fn charge(body: &mut Body, cx: &mut Cx<'_>) {
    if cx.effects.player.motion == Motion::BossEntry {
        return;
    }
    if !cx.world.frame_odd {
        spawn_shot(cx);
        return;
    }
    body.counter.tick_up();
    if body.counter != CHARGED {
        return;
    }
    body.phase = Phase::Charged.byte();
    body.counter = 0;
    body.vx_drift = 0;
    let own = own_slot(cx);
    for slot in 16..=16 + usize::from(own) {
        cx.slots.set_kind(slot, 0);
    }
    spawn_child(body, own, cx);
}

/// `$957E`: a type `$46` in slot 23, one screen below the boss.
fn spawn_child(body: &Body, own: u8, cx: &mut Cx<'_>) {
    let part = cx.slots.main[CHILD_SLOT].spawn(CHILD_KIND, body.x, body.y.wrapping_add(0x0100));
    (part.behaviour, part.subtype, part.phase, part.timer) = (0, 0, 0, 0);
    part.anim = own;
}

/// `$96BB`: a type `$44` shot in the highest free slot 16-23, at one of eight places.
fn spawn_shot(cx: &mut Cx<'_>) {
    let Some(index) = engine::free_child_slot(cx.slots) else {
        return;
    };
    let place = cx.world.rng[0] & 0x07;
    let x = cx.prg.families.stage4_boss.shot_x[usize::from(place)];
    let own = own_slot(cx);
    let child = cx.slots.main[index].spawn(SHOT_KIND, x, 0x0080);
    (child.anim, child.behaviour, child.subtype) = (0, 0, 0);
    (child.phase, child.timer) = (0, 0);
    child.counter = place;
    child.sprite = own;
}

/// `$95B3`: draw the body and the bar, run the boss contact and store.
fn finish(body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
    engine::on_screen(work, cx.world);
    work.pointer = engine::rom_word(cx.prg, HOME_BANK, BODY_SPRITE);
    work.tile = 0;
    engine::draw(work, DRAW_BANK, cx);
    work.pointer = engine::rom_word(
        cx.prg,
        HOME_BANK,
        BODY_SPRITE_BY_TIMER + u16::from(body.timer),
    );
    engine::draw(work, DRAW_BANK, cx);
    draw_bar(body, work, cx);
    work.hitbox = 0;
    let contact = engine::boss_contact(CONTACT, 0, body, work, cx);
    if contact & hit::FLASHING != 0 {
        knock_back(body, work, cx);
    }
    engine::store(body, work);
}

/// `$9616`: the first hit starts the drift away from the nearer edge; every hit lowers the counter.
fn knock_back(body: &mut Body, work: &Work, cx: &Cx<'_>) {
    if body.phase == Phase::Waiting.byte() {
        body.phase = Phase::Drifting.byte();
        let x_high = work.x.to_be_bytes()[0];
        let leftwards = x_high >= 0x0A || (x_high >= 0x07 && cx.world.rng[0] & 0x80 != 0);
        body.vx_drift = if leftwards { -KNOCK } else { KNOCK };
    }
    body.counter = body.counter.saturating_sub(6);
}

/// `$964F`: the bar's frame, then six cells filled from the counter, 8 steps each.
fn draw_bar(body: &Body, work: &mut Work, cx: &mut Cx<'_>) {
    let saved_x = work.screen_x;
    work.pointer = engine::rom_word(cx.prg, HOME_BANK, BAR_FRAME);
    work.screen_x = FRAME_X;
    work.screen_y = FRAME_Y;
    engine::draw(work, DRAW_BANK, cx);
    let mut left = i16::from(body.counter >> 2);
    work.screen_x = FIRST_CELL_X;
    for _ in 0..=LAST_CELL {
        let cell = if left < 0 {
            0
        } else {
            left -= i16::from(CELL_SPACING);
            if left >= 0 {
                FULL_CELL
            } else {
                u8::try_from((left + i16::from(CELL_SPACING)) * 2).expect("< 16")
            }
        };
        work.pointer = engine::rom_word(cx.prg, HOME_BANK, BAR_CELLS + u16::from(cell));
        engine::draw(work, DRAW_BANK, cx);
        work.screen_x = work.screen_x.wrapping_add(CELL_SPACING);
    }
    work.screen_x = saved_x;
    work.screen_y = BAR_Y;
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// The area-14 `$42` slot at TAS 26808, replayed once for the whole module,
    /// with every other slot cleared each loop so the `$44` shots and `$46` child
    /// it spawns never run.
    fn fight() -> Option<(Scene, usize)> {
        static SCENE: Mutex<Option<Scene>> = Mutex::new(None);
        let mut guard = SCENE.lock().expect("the scene lock");
        if guard.is_none() {
            *guard = Scene::tas(26808);
        }
        let mut scene = guard.as_ref()?.clone();
        let boss = (0..24).find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x42)?;
        for slot in 0..24 {
            if slot == boss {
                continue;
            }
            let addr = 0x7CC0 + u16::try_from(slot).expect("slot < 24");
            scene.machine.bus.trace.pokes.push((0xD450, addr, 0));
        }
        Some((scene, boss))
    }

    /// Area 14:00, the stage-4 boss room. The player's fire hits the boss on the
    /// sixth frame, so waiting and drifting both run; the boss draws every frame.
    #[test]
    fn matches_the_oracle() {
        let Some((scene, _)) = fight() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 300, &[0x42]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (300, 300, 300)
        );
        for phase in [0u8, 1] {
            assert!(
                coverage.phases.get(&phase).is_some_and(|&n| n > 0),
                "phase {phase}: {:?}",
                coverage.phases
            );
        }
    }

    /// The charged phase (the counter reached `$C0`) is a counter cycle the 300-frame scene
    /// does not run: the player's hits keep draining `$7E10`. Forcing `$7DD0` each
    /// loop drives the same 6502 path in both machines, so the diff proves it.
    #[test]
    fn the_charged_phase_matches_the_oracle() {
        let Some((mut scene, boss)) = fight() else {
            return;
        };
        let at = 0x7DD0 + u16::try_from(boss).expect("slot < 24");
        scene.machine.bus.trace.pokes.push((0xD450, at, 2));
        let coverage = assert_actor_phases(scene, 200, &[0x42]);
        assert!(
            coverage.phases.get(&2).is_some_and(|&n| n > 0),
            "{:?}",
            coverage.phases
        );
    }

    /// The dying branch `$9482`: with `$7D70` bit 7 set `$AF97` counts `$7F00`
    /// down, the first frame calls `$B1C4` (`boss_defeated`), and the draw is the
    /// bar alone (even frames) or the whole tail (odd frames).
    #[test]
    fn the_flash_matches_the_oracle() {
        let Some((mut scene, boss)) = fight() else {
            return;
        };
        let at = 0x7D70 + u16::try_from(boss).expect("slot < 24");
        scene.machine.bus.trace.pokes.push((0xD450, at, 0x80));
        let coverage = assert_actor_phases(scene, 80, &[0x42]);
        assert_eq!(coverage.occupied, 80);
        assert!(coverage.drawn > 0, "{coverage:?}");
    }

    /// The `$C0` charge transition (`$9566`): it clears `$7CD0` and spawns the
    /// `$46` child into slot 23. The natural scene's hits keep `$7E10` drained, so
    /// hold the counter at `$BF` and the phase at 0; every odd frame completes it.
    #[test]
    fn the_charge_transition_matches_the_oracle() {
        let Some((mut scene, boss)) = fight() else {
            return;
        };
        let at = u16::try_from(boss).expect("slot < 24");
        for (addr, value) in [(0x7DD0, 0u8), (0x7E10, 0xBF)] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, addr + at, value));
        }
        let mut probe = scene.clone();
        let mut child = false;
        for _ in 0..20 {
            probe.run(1, &[]);
            child |= probe.machine.bus.wram[0x1CD7] == 0x46;
        }
        assert!(child, "the $46 child never spawned");
        let coverage = assert_actor_phases(scene, 40, &[0x42]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }

    /// `$AFD6` on the player: standing in the boss is contact, and the oracle
    /// takes the hurt (`$0479` is set to `$20`).
    #[test]
    fn hurts_the_player_up_close() {
        let Some((mut scene, boss)) = fight() else {
            return;
        };
        scene.isolate(&[0x42]);
        scene.run(1, &[]);
        let player = (
            u16::from_be_bytes([scene.machine.bus.ram[0x460], scene.machine.bus.ram[0x461]]),
            u16::from_be_bytes([scene.machine.bus.ram[0x462], scene.machine.bus.ram[0x463]]),
        );
        let at = u16::try_from(boss).expect("slot < 24");
        for (addr, value) in [
            (0x7B00, player.0.to_be_bytes()[0]),
            (0x7B20, player.0.to_be_bytes()[1]),
            (0x7B40, player.1.to_be_bytes()[0]),
            (0x7B60, player.1.to_be_bytes()[1]),
            (0x7EE0, 0),
        ] {
            scene
                .machine
                .bus
                .trace
                .pokes
                .push((0xD450, addr + at, value));
        }
        let mut probe = scene.clone();
        let mut hurt = 0u8;
        for _ in 0..90 {
            probe.run(1, &[]);
            hurt = hurt.max(probe.machine.bus.ram[0x479]);
        }
        assert!(hurt > 0, "the oracle never took boss contact");
        let coverage = assert_actor_phases(scene, 90, &[0x42]);
        assert!(coverage.occupied > 0, "{coverage:?}");
    }
}
