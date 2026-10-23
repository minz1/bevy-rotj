//! Type `$AC` (bank 12 `$89A1`): a stationary enemy that draws one metasprite and,
//! on its `$7E90` timer, drops a type `$24` projectile to the side it faces.
//!
//! The handler never reads `$7DD0` or `$7D50` (the record's state `$84` is only
//! read by the dying path), so its only phase is 0.

use crate::actor::engine::{self, Visibility, Work};
use crate::actor::oam::attr;
use crate::actor::slots::{Body, Field, own_field};
use crate::actor::world::Cx;

/// The metasprite pointer (`$96/$97`), read from bank 12 `$8A50`/`$8A51`: bank 9's `$B14A`.
const POINTER: u16 = 0xB14A;
/// `$81C6` maps bank 9 at `$A000` for the draw.
const DRAW_BANK: u8 = 9;
/// The dying variant the handler passes in `A` to `$A56E`.
const DYING: u8 = 0x80;
/// Hit-box index for the contact check (`$A5`).
const HITBOX: u8 = 0x0B;
/// The child kind written to `$7CD0,y` (`$24`, bank 8 `$9823`).
const CHILD: u8 = 0x24;
/// The child's `$7DD0` phase, right-facing arm.
const RIGHT: u8 = 0x09;
/// The child's `$7DD0` phase, left-facing arm.
const LEFT: u8 = 0x08;
/// The child's x offset from the parent (`$E0` in 1/16 px).
const OFFSET: u16 = 0x00E0;
/// The child's y offset from the parent (`$40` in 1/16 px).
const DROP: u16 = 0x0040;
/// The child's metasprite frame (`$7C30`).
const CHILD_SPRITE: u8 = 6;
/// The timer `$7E90` is reloaded with after a spawn.
const RELOAD: u8 = 0x40;

/// Type `$AC` keeps its whole state in [`Body`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State;

impl State {
    /// One frame of the handler (`$89A1`).
    pub fn step(&mut self, kind: &mut u8, body: &mut Body, work: &mut Work, cx: &mut Cx<'_>) {
        engine::load(body, work);
        if engine::dying(kind, body, work, DYING, cx) {
            return;
        }
        work.attr |= body.flags;
        // Off screen, `$89D9` indexes with the X `$A852` left, not the slot.
        let x_register = if engine::on_screen(work, cx.world) == Visibility::On {
            work.tile = 0;
            work.pointer = POINTER;
            engine::draw(work, DRAW_BANK, cx);
            work.hitbox = HITBOX;
            engine::contact(kind, body, work, 0, cx);
            engine::store(body, work);
            work.slot
        } else {
            engine::off_screen_x(work, cx.world)
        };
        let slot = usize::from(work.slot);
        let mut indexed = Indexed {
            x_register,
            slot,
            body,
        };
        let timer = indexed.byte(0x7E90, cx).wrapping_sub(1);
        indexed.set_byte(0x7E90, timer, cx);
        if timer & 0x80 == 0 {
            return;
        }
        indexed.set_byte(0x7E90, RELOAD, cx);
        let Some(index) = engine::free_child_slot(cx.slots) else {
            return;
        };
        let chr_slot = indexed.byte(0x7C90, cx);
        spawn_child(index, work, chr_slot, cx);
    }
}

/// An `array,x` access with X possibly not the running slot.
struct Indexed<'a> {
    x_register: u8,
    slot: usize,
    body: &'a mut Body,
}

impl Indexed<'_> {
    fn addr(&self, array: u16) -> u16 {
        array + u16::from(self.x_register)
    }

    fn field(array: u16) -> Field {
        Field::from_array(array).expect("own_field returns a Body array")
    }

    fn byte(&self, array: u16, cx: &Cx<'_>) -> u8 {
        let addr = self.addr(array);
        match own_field(addr, self.slot) {
            Some(own) => self.body.get(Self::field(own)),
            None => cx.byte(addr),
        }
    }

    fn set_byte(&mut self, array: u16, value: u8, cx: &mut Cx<'_>) {
        let addr = self.addr(array);
        match own_field(addr, self.slot) {
            Some(own) => self.body.set(Self::field(own), value),
            None => cx.set_byte(addr, value),
        }
    }
}

/// `$89F5`: hands a type `$24` to the free child slot `index`, to the side of the
/// parent that its attribute bit 6 selects.
fn spawn_child(index: usize, work: &Work, chr_slot: u8, cx: &mut Cx<'_>) {
    let flags = (work.attr & attr::FLIP_X) | attr::PALETTE;
    let (phase, x) = if flags & attr::FLIP_X != 0 {
        (RIGHT, work.x.wrapping_add(OFFSET))
    } else {
        (LEFT, work.x.wrapping_sub(OFFSET))
    };
    let child = cx.slots.main[index].spawn(CHILD, x, work.y.wrapping_add(DROP));
    child.flags = flags;
    child.phase = phase;
    child.chr_slot = chr_slot;
    child.state = 0;
    child.sprite = CHILD_SPRITE;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::actor::oracle::{Coverage, Scene, assert_actor_phases};

    /// The `$AC` of area 20:02 (`Scene::tas(38884)`; §8b: slots 1 and 3, `$7D50`
    /// `$84`, contact and weapon).
    ///
    /// Every 65 frames the handler drops a type `$24` into a free slot 16-23; the
    /// child carries `$7C30` = 6 and draws the bank-8 `$98D0` metasprite, which
    /// `enemy_shot` now handles.
    fn scene_at(frame: u64) -> Option<Scene> {
        Scene::tas(frame)
    }

    #[test]
    fn matches_the_oracle() {
        let Some(scene) = scene_at(38884) else {
            return;
        };
        let mut drops = 0;
        let mut probe = scene.clone();
        for _ in 0..70 {
            probe.run(1, &[]);
            drops +=
                usize::from((16..24).any(|slot| probe.machine.bus.wram[0x1CC0 + slot] == 0x24));
        }
        assert!(drops > 0, "the $AC never dropped a $24 child");
        let coverage = assert_actor_phases(scene, 300, &[0xAC]);
        assert_eq!(
            coverage,
            Coverage {
                frames: 300,
                occupied: 300,
                drawn: 300,
                phases: BTreeMap::from([(0, 571)]),
            }
        );
    }

    /// The handler's contact (`$A9C4`) with the player standing on the actor: the
    /// `$A56E`-style hurt sets his `$0479` timer (the damage is 0, so energy does
    /// not move).
    #[test]
    fn touches_the_player_up_close() {
        let Some(mut scene) = scene_at(38884) else {
            return;
        };
        let wram = &scene.machine.bus.wram;
        let slot = (0..16).find(|&s| wram[0x1CC0 + s] == 0xAC).expect("a $AC");
        let x = u16::from_be_bytes([wram[0x1B00 + slot], wram[0x1B20 + slot]]);
        let y = u16::from_be_bytes([wram[0x1B40 + slot], wram[0x1B60 + slot]]);
        scene.player(x, y);
        let mut contact = scene.clone();
        let mut hurt = 0;
        for _ in 0..40 {
            contact.run(1, &[]);
            hurt += usize::from(contact.machine.bus.ram[0x479] != 0);
        }
        assert!(hurt > 0, "the oracle takes no contact from the $AC");
        let coverage = assert_actor_phases(scene, 40, &[0xAC]);
        assert_eq!((coverage.frames, coverage.occupied), (40, 40));
        assert!(coverage.drawn > 0, "{coverage:?}");
        assert!(
            coverage.phases.get(&0).is_some_and(|&n| n > 0),
            "{coverage:?}"
        );
    }
}
