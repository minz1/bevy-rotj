//! Type `$30` (bank 11 `$887A`): the child a `$2E` lowers, then throws at the
//! player.
//!
//! It flies with the velocity the throw gave it (`dropper`) until it drops to
//! the player's y, then hovers for eight frames of a landing animation and frees
//! itself, clearing its parent's phase.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Field};
use crate::actor::world::{Cx, sound};

const BANK: u8 = 11;
/// The two bytes of the resting pose's metasprite pointer (`$893C`).
const REST_POINTER: u16 = 0x893C;
/// The four landing-pose pointers (`$893E`), indexed by the animation byte.
const FALL_POINTERS: u16 = 0x893E;
const CHR: u8 = 0xAF;
const HITBOX: u8 = 4;
const DAMAGE: u8 = 2;
/// The animation byte the landing pose ends at.
const END: u8 = 0x88;
const PARENT: u8 = 0x2E;

/// `$7DD0` bit 7, set by the `$2E` when it throws: until then the child does nothing.
const THROWN: u8 = 0x80;
/// `anim` bit 7 from the landing on; the bits below count the landing pose's frames.
const LANDING: u8 = 0x80;

/// Type `$30` keeps its parent's slot in `subtype`, the y it lands at in `counter:timer`
/// and the landing in `anim`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$887A`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        if body.phase & THROWN == 0 {
            return;
        }
        engine::load(body, work);
        cx.oam.chr[usize::from(body.chr_slot)] = CHR;
        work.attr = body.flags;
        engine::integrate(body, work);
        let landing_y = u16::from_be_bytes([body.counter, body.timer]);
        if work.y.to_be_bytes()[0] & 0x80 == 0 && work.y >= landing_y && body.anim & LANDING == 0 {
            cx.effects.sound[0] = sound::IMPACT;
            body.flags &= attr::PALETTE;
            work.attr = body.flags;
            body.vx = 0;
            body.anim = LANDING;
            work.y = work.y.wrapping_add(0x0100);
        }
        if engine::on_screen(work, cx.world) != Visibility::On {
            tail(body, work);
            return;
        }
        if body.anim & LANDING == 0 {
            work.hitbox = 0;
            work.pointer = engine::rom_word(cx.prg, BANK, REST_POINTER);
        } else {
            if body.anim >= END {
                *kind = 0;
                let parent = usize::from(body.subtype);
                if cx.slots.kind(parent) == PARENT {
                    cx.set(parent, Field::Phase, 0);
                }
                return;
            }
            work.pointer =
                engine::rom_word(cx.prg, BANK, FALL_POINTERS + u16::from(body.anim & 0x06));
            work.hitbox = HITBOX;
            body.anim = body.anim.wrapping_add(1);
        }
        engine::tile_bits(work, cx.prg);
        engine::draw(work, 1, cx);
        engine::touch(work, DAMAGE, cx);
        tail(body, work);
    }
}

/// The throw (`$87D3`): the `$2E` in `parent` sends the child from `y` at `vx`, to land
/// at `landing_y`.
pub(crate) fn arm(child: &mut Body, parent: u8, landing_y: u16, y: u16, vx: i16) {
    let [landing_high, landing_low] = landing_y.to_be_bytes();
    child.phase = THROWN;
    child.y = y;
    child.vy = -0x40;
    child.anim = 0;
    child.ref_vx = 0;
    child.vx_drift = 0;
    child.state = 0;
    child.hit = 0;
    child.subtype = parent;
    child.contact = 0x20;
    child.timer = landing_low;
    child.counter = landing_high;
    child.vx = vx;
}

/// `$8921`: no x acceleration, the landing holds its height, then the store.
fn tail(body: &mut Body, work: &mut Work) {
    work.ax = 0;
    if body.anim & LANDING != 0 {
        work.vy = -6;
    }
    engine::end_frame(body, work);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The one `$30` the `$2E` of area 0C:00 threw, and the slot it names as parent.
    fn child_and_parent(scene: &Scene) -> (u16, u16) {
        let wram = &scene.machine.bus.wram;
        let child = (0..32).find(|&s| wram[0x1CC0 + s] == 0x30).expect("a $30");
        let child = u16::try_from(child).expect("< 32");
        (child, u16::from(wram[0x1DB0 + usize::from(child)]))
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = Scene::tas(13670) else {
            return;
        };
        let coverage = assert_actor_phases(scene, 60, &[0x30]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 60,
                occupied: 37,
                drawn: 36,
                phases: BTreeMap::from([(0x80, 37)]),
            }
        );
    }

    /// The TAS child frees with its `$2E` parent alive, so this runs the parent too.
    /// Its phase-4 timer is 63 at frame 13670 and 27 when the child frees, so the
    /// parent's phase 4 -> 0 is the child's `$7DD0,y` clear, not the timer.
    #[test]
    fn the_child_clears_its_live_parent_phase() {
        let Some(scene) = Scene::tas(13670) else {
            return;
        };
        let (_, parent) = child_and_parent(&scene);
        assert_eq!(scene.machine.bus.wram[0x1CC0 + usize::from(parent)], 0x2E);
        assert!(scene.machine.bus.wram[0x1DF0 + usize::from(parent)] > 37);
        let coverage = assert_actor_phases(scene, 60, &[0x2E, 0x30]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 60,
                occupied: 60,
                drawn: 60,
                phases: BTreeMap::from([(0, 23), (4, 37), (0x80, 37)]),
            }
        );
    }

    /// The handler's only dispatch is `$7DD0` bit 7; the child never runs before the
    /// throw sets `$80` because the spawner takes the highest free slot, above its
    /// parent. A forced clear bit leaves the slot untouched.
    #[test]
    fn a_clear_phase_bit_is_dormant() {
        let Some(mut scene) = Scene::tas(13670) else {
            return;
        };
        let (child, _) = child_and_parent(&scene);
        scene
            .machine
            .bus
            .trace
            .pokes
            .push((0xD450, 0x7DD0 + child, 0));
        let coverage = assert_actor_phases(scene, 20, &[0x30]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 20,
                occupied: 20,
                drawn: 0,
                phases: BTreeMap::from([(0, 20)]),
            }
        );
    }
}
