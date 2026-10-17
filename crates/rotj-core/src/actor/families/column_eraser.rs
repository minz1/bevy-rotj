//! Type `$56`, the leftover-column eraser (bank 11 `$9DF9`): each frame it steps `$7DD0` by
//! `$10` and zeroes those 16 map columns in all 16 rows, then frees its slot when `$7DD0`
//! wraps to 0. It never draws and never touches the player.

use crate::actor::engine::Work;
use crate::actor::slots::Body;
use crate::actor::world::Cx;

/// Type `$56` keeps no state outside [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$9DF9`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, _work: &mut Work, cx: &mut Cx<'_>) {
        body.phase = body.phase.wrapping_add(0x10);
        if body.phase == 0 {
            *kind = 0;
            return;
        }
        let first = usize::from(body.phase);
        for row in cx.effects.level.map.as_chunks_mut::<256>().0 {
            row[first..first + 16].fill(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::oracle::{Scene, State};
    use crate::actor::slots::{load_aliases, store_aliases};
    use crate::actor::world::Cx;

    /// TAS frames 8600 (area 08:00) and 28835 (area 18:00) are where the `$56` is first
    /// seen; the second scene starts earlier to catch the whole erase. Its 16 calls run through an NMI (the erase loop is ~800 instructions), which
    /// `assert_routine` rejects, so each call from `$9DF9` to an exit (`$9E09` frees,
    /// `$9E20` erases) is replayed here and diffed on what the handler owns: the level map
    /// `$6000-$6FFF` and the slot's `$7CC0`/`$7DD0`. The first 15 calls erase column groups
    /// `$10`-`$F0`; the 16th wraps `$7DD0` to 0 and frees the slot.
    #[test]
    fn matches_the_oracle() {
        for (frame, start) in [(8600, 8599), (28835, 28790)] {
            let Some(mut scene) = Scene::tas(start) else {
                return;
            };
            let prg = scene.prg();
            let events = scene.run(80, &[0x9DF9, 0x9E09, 0x9E20]);
            let mut checked = 0;
            for pair in events.windows(2) {
                let (begin, end) = (&pair[0], &pair[1]);
                if begin.pc != 0x9DF9 || end.pc == 0x9DF9 || begin.bank != Some(11) {
                    continue;
                }
                let slot = usize::from(begin.ram[0x10]);
                assert_eq!(slot, 1);
                let mut state = State::lift(begin);
                let mut taken = std::mem::take(&mut state.slots.main[slot]);
                state.slots.main[slot].kind = taken.kind;
                load_aliases(
                    &mut taken.body,
                    slot,
                    &state.slots,
                    &state.effects.projectiles,
                );
                let mut cx = Cx {
                    prg: &prg,
                    world: &state.world,
                    slots: &mut state.slots,
                    schedule: &mut state.schedule,
                    effects: &mut state.effects,
                    scroll: &mut state.scroll,
                    oam: &mut state.oam,
                };
                super::State.step(&mut taken.kind, &mut taken.body, &mut state.work, &mut cx);
                store_aliases(
                    &taken.body,
                    slot,
                    &mut state.slots,
                    &mut state.effects.projectiles,
                );
                state.slots.main[slot] = taken;
                let (_, wram) = state.project(begin);
                assert_eq!(
                    wram[..0x1000],
                    end.wram[..0x1000],
                    "frame {frame} call {checked}"
                );
                assert_eq!(
                    wram[0x1CC1], end.wram[0x1CC1],
                    "frame {frame} call {checked}"
                );
                assert_eq!(
                    wram[0x1DD1], end.wram[0x1DD1],
                    "frame {frame} call {checked}"
                );
                assert_eq!(end.pc == 0x9E09, checked == 15, "frame {frame}");
                checked += 1;
            }
            assert_eq!(checked, 16, "frame {frame}");
            let wram = &scene.machine.bus.wram;
            assert!(
                wram[..0x1000]
                    .as_chunks::<256>()
                    .0
                    .iter()
                    .all(|row| row[16..].iter().all(|&id| id == 0)),
                "frame {frame}"
            );
        }
    }
}
