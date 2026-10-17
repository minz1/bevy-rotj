//! Type `$C4` (bank 12 `$9897`): a drifting projectile the area-20 `$C2`
//! controller drops into the high main slots (`$97F0`). Each frame it adds a
//! per-subtype velocity from `$990B` to its `$7E60:$7E50`/`$7E80:$7E70`, steps
//! through `$A79B` (`integrate`), projects (`$A852`), and if on screen draws one
//! of two bank-12 metasprites, selected by `$F1` bit 2, then contacts the player
//! (`$A9C4`, damage 2). Off screen it frees its own slot.
//!
//! It calls only routines in `docs/ACTORS.md` §9c: `$A000` (`load`), `$A79B`
//! (`integrate`), `$A852` (`on_screen`), `$C247` (`draw`), `$A9C4` (`contact`)
//! and `$A934` (`store`). The draw reaches `$C247` through the bank-12 `$81C6`
//! wrapper, but that wrapper only maps bank 9 at `$A000`; the metasprite pointer
//! `$991B` holds is a `$8000`-window address, so the metasprite is read from the
//! handler's own bank 12 (bank 9's same offset is `$FF`).
//!
//! It dispatches on no `$7DD0`/`$7D50`, so its phase is whatever the spawn left.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// The handler's own bank, mapped at `$8000` by the actor loop.
const HOME_BANK: u8 = 12;
/// The per-subtype velocity change (`$990B`, four bytes: `$7E60`, `$7E50`,
/// `$7E80`, `$7E70`): a signed 16-bit add to `vx_drift` then to `vy`.
const DRIFT: u16 = 0x990B;
/// The two metasprite pointers (`$991B`), picked by `$F1` bit 2.
const SHEET: u16 = 0x991B;
/// CHR shadow the draw selects (`$98`).
const CHR_SLOT: u8 = 6;
/// Tile bits for the draw (`$A4`).
const TILE: u8 = 0x80;
/// Hit-box index (`$A5`).
const HITBOX: u8 = 0x0C;
/// The damage a touch deals (`A` before `$A9C4`).
const DAMAGE: u8 = 2;

/// Type `$C4` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9897`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        work.attr = body.flags;
        engine::integrate(body, work);
        let at = DRIFT + 4 * u16::from(body.subtype);
        let prg = cx.prg;
        let delta = |offset: u16| {
            i16::from_le_bytes([
                engine::rom_byte(prg, HOME_BANK, at + offset),
                engine::rom_byte(prg, HOME_BANK, at + offset + 1),
            ])
        };
        body.vx_drift = body.vx_drift.wrapping_add(delta(0));
        body.vy = body.vy.wrapping_add(delta(2));
        if engine::on_screen(work, cx.world) != Visibility::On {
            *kind = 0;
            return;
        }
        let frame = u16::from((cx.world.frame >> 1) & 0x02);
        work.pointer = engine::rom_word(prg, HOME_BANK, SHEET + frame);
        work.chr_slot = CHR_SLOT;
        work.tile = TILE;
        engine::draw(work, HOME_BANK, cx);
        work.hitbox = HITBOX;
        engine::contact(kind, body, work, DAMAGE, cx);
        engine::store(body, work);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The area-20 `$C2` controller spawns one `$C4` at a time (`$97F0`); the
    /// spawner is unported, so isolating the kind leaves the actors alive at the
    /// lift. They all die off screen; 380 frames cover one whole life and the
    /// freed tail. One phase (the spawn value 0), drawn all but the freeing loop.
    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(38250) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 380, &[0xC4]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 380,
                occupied: 36,
                drawn: 35,
                phases: BTreeMap::from([(0, 36)]),
            },
            "{coverage:?}"
        );
    }

    /// The scene only ever seeds subtypes 0 and 1; the drift table `$990B` has
    /// four entries (subtype 2's is the one negative `vy` step). Forcing every
    /// live `$C4`'s `$7DB0` in turn exercises all four indexes.
    #[test]
    fn every_subtype_drift_matches_the_oracle() {
        let Some(scene) = Scene::tas(38250) else {
            return;
        };
        let slots: Vec<u16> = (0..16)
            .filter(|&slot| scene.machine.bus.wram[0x1CC0 + slot] == 0xC4)
            .map(|slot| u16::try_from(slot).expect("a slot"))
            .collect();
        assert!(!slots.is_empty(), "no live $C4");
        let expected = [
            Coverage {
                frames: 36,
                occupied: 36,
                drawn: 35,
                phases: BTreeMap::from([(0, 36)]),
            },
            Coverage {
                frames: 36,
                occupied: 36,
                drawn: 35,
                phases: BTreeMap::from([(0, 36)]),
            },
            Coverage {
                frames: 36,
                occupied: 28,
                drawn: 25,
                phases: BTreeMap::from([(0, 28)]),
            },
            Coverage {
                frames: 36,
                occupied: 36,
                drawn: 35,
                phases: BTreeMap::from([(0, 36)]),
            },
        ];
        let mut seen = Vec::new();
        for subtype in 0u8..4 {
            let mut scene = scene.clone();
            for &slot in &slots {
                scene
                    .machine
                    .bus
                    .trace
                    .pokes
                    .push((0xD450, 0x7DB0 + slot, subtype));
            }
            seen.push((subtype, assert_actor_phases(scene, 36, &[0xC4])));
        }
        for (subtype, coverage) in seen {
            assert_eq!(
                coverage,
                expected[usize::from(subtype)],
                "subtype {subtype}"
            );
        }
    }

    /// The actor's `$7EE0` contact byte is `$88`, so `$A9C4` runs the player
    /// check. The TAS player never touches the projectile (it flies past), so the
    /// test pins it onto him at `$D450` each frame and clears `$0472` bit 7 and
    /// `$047A` bit 6, which gate the check off as `ledge_walker`/`hop_thrower` do. Energy drops by
    /// 2 a frame until it wraps, so `hits` counts the seven hits before the wrap.
    #[test]
    fn hurts_the_player_up_close() {
        let Some(mut scene) = Scene::tas(38250) else {
            return;
        };
        scene.isolate(&[0xC4]);
        let slot = (0..16)
            .find(|&s| scene.machine.bus.wram[0x1CC0 + s] == 0xC4)
            .expect("a live $C4");
        let slot = u16::try_from(slot).expect("a slot");
        let mut probe = scene.clone();
        let mut energy = probe.machine.bus.ram[0x478];
        let mut hits = 0;
        for _ in 0..30 {
            let ram = &probe.machine.bus.ram;
            let x = u16::from_be_bytes([ram[0x460], ram[0x461]]);
            let y = u16::from_be_bytes([ram[0x462], ram[0x463]]);
            let [xh, xl] = x.to_be_bytes();
            let [yh, yl] = y.to_be_bytes();
            probe.machine.bus.trace.pokes = vec![
                (0xD450, 0x0472, 0),
                (0xD450, 0x047A, 0),
                (0xD450, 0x7B00 + slot, xh),
                (0xD450, 0x7B20 + slot, xl),
                (0xD450, 0x7B40 + slot, yh),
                (0xD450, 0x7B60 + slot, yl),
            ];
            probe.run(1, &[]);
            let now = probe.machine.bus.ram[0x478];
            if now < energy {
                hits += 1;
                energy = now;
            }
        }
        assert!(hits > 0, "the oracle takes no contact hit");
        let coverage = assert_actor_phases(scene, 30, &[0xC4]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 30,
                occupied: 30,
                drawn: 30,
                phases: BTreeMap::from([(0, 30)]),
            },
            "{coverage:?}"
        );
    }
}
