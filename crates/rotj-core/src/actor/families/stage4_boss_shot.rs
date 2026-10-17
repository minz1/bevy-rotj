//! Type `$44` (bank 11 `$9739`): the stage-4 boss's shot. It accelerates on a
//! per-shot table chosen at spawn (`$7E10`), integrates its own offset, draws
//! offset by the slot in `$7C30` (the `$42` that fired it), and frees itself
//! after eight frames. It calls only `$A000` (`load`), `$A852` (`on_screen`) and
//! `$C247` (`draw`, bank 9 through the bank-11 wrapper `$826F`); no contact.

use crate::actor::engine::{self, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

const HOME_BANK: u8 = 11;
const DRAW_BANK: u8 = 9;
/// X velocity per frame by the spawn counter: low (`$97DF`) and high (`$97E7`).
const X_ACCEL_LOW: u16 = 0x97DF;
const X_ACCEL_HIGH: u16 = 0x97E7;
/// Y velocity per frame by the spawn counter (`$97EF`), with a fixed `-1` high byte.
const Y_ACCEL_LOW: u16 = 0x97EF;
/// Four metasprites by `$7E90 & $FE` (`$97F7`).
const METASPRITES: u16 = 0x97F7;
const LIFE: u8 = 0x08;

/// Type `$44` keeps its x velocity in the pair `[subtype, behaviour]`, its y velocity in
/// `[timer, phase]`, its table in `counter`, its age in `anim` and its parent's slot in
/// `sprite`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9739`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        let index = u16::from(body.counter);
        let accel = |table: u16| engine::rom_byte(cx.prg, HOME_BANK, table + index);
        let x_accel = u16::from_be_bytes([accel(X_ACCEL_HIGH), accel(X_ACCEL_LOW)]);
        let x_velocity = u16::from_be_bytes([body.subtype, body.behaviour]).wrapping_add(x_accel);
        [body.subtype, body.behaviour] = x_velocity.to_be_bytes();
        let y_velocity = u16::from_be_bytes([body.timer, body.phase])
            .wrapping_add(u16::from_be_bytes([0xFF, accel(Y_ACCEL_LOW)]));
        [body.timer, body.phase] = y_velocity.to_be_bytes();

        work.x = work.x.wrapping_add(x_velocity);
        body.x = work.x;
        work.y = work.y.wrapping_add(y_velocity);
        body.y = work.y;

        let parent = &cx.slots.main[usize::from(body.sprite)].body;
        work.x = work.x.wrapping_add(parent.x);
        work.y = work.y.wrapping_add(parent.y);

        work.pointer =
            engine::rom_word(cx.prg, HOME_BANK, METASPRITES + u16::from(body.anim & 0xFE));
        engine::on_screen(work, cx.world);
        work.attr = 0;
        work.tile = 0;
        engine::draw(work, DRAW_BANK, cx);

        body.anim = body.anim.wrapping_add(1);
        if body.anim >= LIFE {
            *kind = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::actor::oracle::{Scene, assert_actor_phases};

    /// Area 14:00, the stage-4 boss room, replayed once for the whole module.
    fn fight() -> Option<Scene> {
        static SCENE: Mutex<Option<Scene>> = Mutex::new(None);
        let mut guard = SCENE.lock().expect("the scene lock");
        if guard.is_none() {
            *guard = Scene::tas(26381);
        }
        Some(guard.as_ref()?.clone())
    }

    /// The `$42` stays alive so it keeps spawning shots; every other kind (the
    /// `$46` child included) is isolated away. The `$44` y velocity is the only
    /// source of phases above `$02`, and it takes sixteen distinct values.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = fight() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x42, 0x44]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (120, 120, 120)
        );
        let shots = coverage
            .phases
            .keys()
            .filter(|&&phase| phase > 0x02)
            .count();
        assert!(shots >= 16, "{:?}", coverage.phases);
    }

    /// The `$42` isolated away, so only the shots count: the one alive at frame
    /// 26381 (already one frame old) runs its remaining seven frames, drawn each.
    #[test]
    fn matches_the_oracle_alone() {
        let Some(scene) = fight() else {
            return;
        };
        let coverage = assert_actor_phases(scene, 120, &[0x44]);
        assert_eq!(
            (coverage.frames, coverage.occupied, coverage.drawn),
            (120, 7, 7)
        );
        assert_eq!(coverage.phases.len(), 7, "{:?}", coverage.phases);
    }

    /// Every one of the eight acceleration tables (`$7E10` 0-7) runs under the
    /// oracle diff: the counter of the live shot is pinned to each value.
    #[test]
    fn every_shot_table_runs() {
        let Some(scene) = fight() else {
            return;
        };
        let slot = (16..24)
            .find(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0x44)
            .expect("a $44 shot at frame 26381");
        let mut ran = 0;
        for counter in 0u8..8 {
            let mut scene = scene.clone();
            scene.machine.bus.trace.pokes.push((
                0xD450,
                0x7E10 + u16::try_from(slot).expect("slot < 24"),
                counter,
            ));
            let coverage = assert_actor_phases(scene, 8, &[0x44]);
            ran += usize::from(coverage.occupied > 0);
        }
        assert_eq!(ran, 8, "an acceleration table never ran");
    }
}
