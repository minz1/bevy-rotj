//! Actor slots: the 24 main slots the actor loop runs, and the 8 second-class slots.

use crate::actor::families::Brain;
use crate::actor::world::{PROJECTILE_ARRAYS, Projectile};
use crate::actors::ActorSpawn;

/// The per-slot arrays the spawners and the engine share (`docs/ACTORS.md` §9).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Body {
    /// Level x (`$7B00:$7B20`).
    pub x: u16,
    /// Level y (`$7B40:$7B60`).
    pub y: u16,
    /// Sprite flags (`$7C00`).
    pub flags: u8,
    /// CHR shadow the actor's metasprites select (`$7C90`).
    pub chr_slot: u8,
    /// State byte (`$7D50`).
    pub state: u8,
    /// Behaviour index (`$7D90`).
    pub behaviour: u8,
    /// Subtype index (`$7DB0`).
    pub subtype: u8,
    /// Hit flag (`$7D70`).
    pub hit: u8,
    /// Handler phase (`$7DD0`).
    pub phase: u8,
    /// Handler timer (`$7DF0`).
    pub timer: u8,
    /// Handler counter (`$7E10`).
    pub counter: u8,
    /// Animation counter (`$7E90`).
    pub anim: u8,
    /// Metasprite frame the handler draws (`$7C30`); `$44` and `$C0` store a parent slot.
    pub sprite: u8,
    /// Schedule cell it spawned from (`$7CE0`).
    pub cell: u8,
    /// Frames of the dying animation (`$7F00`).
    pub dying_timer: u8,
    /// A handler-local byte (`$7D30`): ride width, a type-`$08` part's y byte or `$B4`'s toggle.
    pub scratch: u8,
    /// Hit points (`$7EF0:$7BE0`).
    pub health: u16,
    /// Contact flags (`$7EE0`).
    pub contact: u8,
    /// Frames lived (`$7BA0`).
    pub age: u8,
    /// The x velocity bounces are measured against (`$7ED0:$7EC0`).
    pub ref_vx: i16,
    /// X velocity (`$7E30:$7E40`).
    pub vx: i16,
    /// Added to the x step each frame (`$7E50:$7E60`).
    pub vx_drift: i16,
    /// Y velocity (`$7E70:$7E80`).
    pub vy: i16,
}

/// The arrays every main slot keeps (32 wide).
pub const WIDE: [u16; 19] = [
    0x7B00, 0x7B20, 0x7B40, 0x7B60, 0x7C00, 0x7C90, 0x7D50, 0x7D90, 0x7DB0, 0x7D70, 0x7DD0, 0x7DF0,
    0x7E10, 0x7E90, 0x7C30, 0x7CE0, 0x7F00, 0x7D30, 0x7BE0,
];

/// The arrays only slots 0-15 keep (16 wide); slots 16-23 index the next array.
pub const NARROW: [u16; 11] = [
    0x7EE0, 0x7BA0, 0x7EF0, 0x7E30, 0x7E40, 0x7E50, 0x7E60, 0x7E70, 0x7E80, 0x7ED0, 0x7EC0,
];

const KINDS: u16 = 0x7CC0;
/// `$7F70-$7F8F`: no array, but an index left over from `$A852` reaches them (type `$AC`).
const SPARE: u16 = 0x7F70;

/// The player's weight on a second-class platform, one byte per slot (`$7F10`).
pub(crate) const PRESSED: u16 = 0x7F10;

/// Who holds a slot-array byte.
enum Owner {
    Kind(usize),
    Slot(usize, u16),
    Projectile(usize, u16),
    Spare(usize),
}

/// The owner of `addr`: the highest array base at or below it whose width covers it.
fn owner(addr: u16) -> Owner {
    find_owner(addr).unwrap_or_else(|| panic!("${addr:04X} is in no slot array"))
}

fn find_owner(addr: u16) -> Option<Owner> {
    let arrays = WIDE
        .iter()
        .map(|&a| (a, 32))
        .chain(NARROW.iter().map(|&a| (a, 16)))
        .chain(PROJECTILE_ARRAYS.iter().map(|&a| (a, 16)))
        .chain([(KINDS, 32), (SPARE, 32)]);
    let (base, width) = arrays
        .filter(|&(base, _)| base <= addr)
        .max_by_key(|&(base, _)| base)?;
    let offset = usize::from(addr - base);
    if offset >= width {
        return None;
    }
    Some(if base == KINDS {
        Owner::Kind(offset)
    } else if base == SPARE {
        Owner::Spare(offset)
    } else if PROJECTILE_ARRAYS.contains(&base) {
        Owner::Projectile(offset, base)
    } else {
        Owner::Slot(offset, base)
    })
}

/// The byte at a slot-array address, wherever the game's layout puts it.
#[must_use]
pub fn read(slots: &Slots, projectiles: &[Projectile; 16], addr: u16) -> u8 {
    match owner(addr) {
        Owner::Projectile(index, array) => projectiles[index].byte(array),
        _ => slots.byte(addr),
    }
}

/// [`read`], or `None` for an address no slot array owns.
#[must_use]
pub fn owned(slots: &Slots, projectiles: &[Projectile; 16], addr: u16) -> Option<u8> {
    find_owner(addr).map(|_| read(slots, projectiles, addr))
}

/// Writes the byte at a slot-array address.
pub fn write(slots: &mut Slots, projectiles: &mut [Projectile; 16], addr: u16, value: u8) {
    match owner(addr) {
        Owner::Projectile(index, array) => projectiles[index].set_byte(array, value),
        _ => slots.set_byte(addr, value),
    }
}

impl Slots {
    /// The byte at a slot-array address; panics on the weapon arrays (see [`read`]).
    #[must_use]
    pub fn byte(&self, addr: u16) -> u8 {
        match owner(addr) {
            Owner::Kind(slot) if slot < 24 => self.main[slot].kind,
            Owner::Kind(slot) => self.second[slot - 24].kind,
            Owner::Slot(slot, array) if slot < 24 => self.main[slot].body.byte(array),
            Owner::Slot(slot, array) => self.second[slot - 24].body.byte(array),
            Owner::Spare(at) => self.spare[at],
            Owner::Projectile(..) => panic!("${addr:04X} is a weapon array"),
        }
    }

    /// Writes the byte at a slot-array address; panics on the weapon arrays.
    pub fn set_byte(&mut self, addr: u16, value: u8) {
        match owner(addr) {
            Owner::Kind(slot) if slot < 24 => self.main[slot].kind = value,
            Owner::Kind(slot) => self.second[slot - 24].kind = value,
            Owner::Slot(slot, array) if slot < 24 => self.main[slot].body.set_byte(array, value),
            Owner::Slot(slot, array) => self.second[slot - 24].body.set_byte(array, value),
            Owner::Spare(at) => self.spare[at] = value,
            Owner::Projectile(..) => panic!("${addr:04X} is a weapon array"),
        }
    }
}

/// The array of `slot`'s own field at `addr`, if it is one: a handler reads its own
/// fields from its [`Body`], which `Cx::slots` does not hold while it runs.
#[must_use]
pub fn own_field(addr: u16, slot: usize) -> Option<u16> {
    match find_owner(addr)? {
        Owner::Slot(owner, array) if owner == slot => Some(array),
        _ => None,
    }
}

/// Fills a slot 16-23's 16-wide fields from the arrays they alias.
pub(crate) fn load_aliases(
    body: &mut Body,
    index: usize,
    slots: &Slots,
    projectiles: &[Projectile; 16],
) {
    if index >= 16 {
        for array in NARROW {
            body.set_byte(array, read(slots, projectiles, array + slot_offset(index)));
        }
    }
}

/// Writes a slot 16-23's 16-wide fields back into the arrays they alias.
pub(crate) fn store_aliases(
    body: &Body,
    index: usize,
    slots: &mut Slots,
    projectiles: &mut [Projectile; 16],
) {
    if index >= 16 {
        for array in NARROW {
            write(
                slots,
                projectiles,
                array + slot_offset(index),
                body.byte(array),
            );
        }
    }
}

fn slot_offset(index: usize) -> u16 {
    u16::try_from(index).expect("a slot index")
}

fn hi(value: u16) -> u8 {
    value.to_be_bytes()[0]
}

fn lo(value: u16) -> u8 {
    value.to_be_bytes()[1]
}

impl Body {
    /// The slot's byte in `array` (one of [`WIDE`] or [`NARROW`]).
    #[must_use]
    pub fn byte(&self, array: u16) -> u8 {
        match array {
            0x7B00 => hi(self.x),
            0x7B20 => lo(self.x),
            0x7B40 => hi(self.y),
            0x7B60 => lo(self.y),
            0x7C00 => self.flags,
            0x7C90 => self.chr_slot,
            0x7D50 => self.state,
            0x7D90 => self.behaviour,
            0x7DB0 => self.subtype,
            0x7D70 => self.hit,
            0x7DD0 => self.phase,
            0x7DF0 => self.timer,
            0x7E10 => self.counter,
            0x7E90 => self.anim,
            0x7C30 => self.sprite,
            0x7CE0 => self.cell,
            0x7F00 => self.dying_timer,
            0x7D30 => self.scratch,
            0x7EE0 => self.contact,
            0x7BA0 => self.age,
            0x7EF0 => hi(self.health),
            0x7BE0 => lo(self.health),
            0x7E30 => hi(self.vx.cast_unsigned()),
            0x7E40 => lo(self.vx.cast_unsigned()),
            0x7E50 => hi(self.vx_drift.cast_unsigned()),
            0x7E60 => lo(self.vx_drift.cast_unsigned()),
            0x7E70 => hi(self.vy.cast_unsigned()),
            0x7E80 => lo(self.vy.cast_unsigned()),
            0x7ED0 => hi(self.ref_vx.cast_unsigned()),
            0x7EC0 => lo(self.ref_vx.cast_unsigned()),
            other => panic!("${other:04X} is not a Body array"),
        }
    }

    /// Writes the slot's byte in `array`, as `$AC16` writes parts generically.
    pub fn set_byte(&mut self, array: u16, value: u8) {
        let set_hi = |word: u16| u16::from_be_bytes([value, lo(word)]);
        let set_lo = |word: u16| u16::from_be_bytes([hi(word), value]);
        let set_hi_i = |word: i16| set_hi(word.cast_unsigned()).cast_signed();
        let set_lo_i = |word: i16| set_lo(word.cast_unsigned()).cast_signed();
        match array {
            0x7B00 => self.x = set_hi(self.x),
            0x7B20 => self.x = set_lo(self.x),
            0x7B40 => self.y = set_hi(self.y),
            0x7B60 => self.y = set_lo(self.y),
            0x7C00 => self.flags = value,
            0x7C90 => self.chr_slot = value,
            0x7D50 => self.state = value,
            0x7D90 => self.behaviour = value,
            0x7DB0 => self.subtype = value,
            0x7D70 => self.hit = value,
            0x7DD0 => self.phase = value,
            0x7DF0 => self.timer = value,
            0x7E10 => self.counter = value,
            0x7E90 => self.anim = value,
            0x7C30 => self.sprite = value,
            0x7CE0 => self.cell = value,
            0x7F00 => self.dying_timer = value,
            0x7D30 => self.scratch = value,
            0x7EE0 => self.contact = value,
            0x7BA0 => self.age = value,
            0x7EF0 => self.health = set_hi(self.health),
            0x7BE0 => self.health = set_lo(self.health),
            0x7E30 => self.vx = set_hi_i(self.vx),
            0x7E40 => self.vx = set_lo_i(self.vx),
            0x7E50 => self.vx_drift = set_hi_i(self.vx_drift),
            0x7E60 => self.vx_drift = set_lo_i(self.vx_drift),
            0x7E70 => self.vy = set_hi_i(self.vy),
            0x7E80 => self.vy = set_lo_i(self.vy),
            0x7ED0 => self.ref_vx = set_hi_i(self.ref_vx),
            0x7EC0 => self.ref_vx = set_lo_i(self.ref_vx),
            other => panic!("${other:04X} is not a Body array"),
        }
    }
}

impl Body {
    /// Writes a spawn record as the spawners do for slots 0-15; other fields keep residue.
    pub fn spawn(&mut self, record: &ActorSpawn, cell: u8) {
        let r = record.raw;
        self.x = u16::from_be_bytes([r[0], r[1]]);
        self.y = u16::from_be_bytes([r[2], r[3]]);
        self.flags = r[4] & 0xE3;
        self.chr_slot = (r[4] >> 2) & 7;
        self.state = r[6];
        self.behaviour = r[7];
        self.subtype = r[8];
        self.hit = 0;
        self.phase = 0;
        self.timer = 0;
        self.counter = 0;
        self.anim = 0;
        self.sprite = 0;
        self.vx = 0;
        self.vx_drift = 0;
        self.vy = 6;
        self.age = 0;
        self.ref_vx = 0;
        self.health = u16::from_be_bytes([r[9], r[10]]);
        self.contact = r[11];
        self.cell = cell;
    }
}

/// One main slot. A freed slot keeps its body, as the game keeps the bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slot {
    /// Occupant type (`$7CC0`), 0 when free.
    pub kind: u8,
    /// Shared state.
    pub body: Body,
    /// The family's own state.
    pub brain: Brain,
}

/// Which routine spawned an actor; they differ in what they clear in slots 24-31.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spawner {
    /// The scrolling column spawn (bank 2 `$87C5`).
    Column,
    /// The room-start spawn (bank 14 `$D714`).
    Room,
}

/// A spawn the port made into a second-class slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Schedule cell.
    pub cell: u8,
    /// The record written.
    pub record: ActorSpawn,
    /// The routine that wrote it.
    pub by: Spawner,
}

/// A second-class slot (24-31), run by `$CE7F` over the 32-wide arrays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Second {
    /// Occupant type (`$7CD8`), 0 when free.
    pub kind: u8,
    /// The record the port spawned into it, for reference.
    pub placed: Option<Placed>,
    /// Its 32-wide arrays.
    pub body: Body,
    /// Its family's state.
    pub brain: Brain,
}

/// Every actor slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slots {
    /// Slots 0-23, run by the actor loop.
    pub main: [Slot; 24],
    /// Slots 24-31.
    pub second: [Second; 8],
    /// `$7F70-$7F8F`, which no array owns.
    pub spare: [u8; 32],
}

#[cfg(test)]
mod layout {
    use super::{Body, NARROW, Second, Slot, Slots, WIDE};

    pub(crate) fn at(addr: u16, slot: usize) -> usize {
        usize::from(addr - 0x6000) + slot
    }

    impl Body {
        pub(crate) fn lift(wram: &[u8; 8192], slot: usize) -> Body {
            let mut body = Body::default();
            let narrow = if slot < 16 { &NARROW[..] } else { &[] };
            for &array in WIDE.iter().chain(narrow) {
                body.set_byte(array, wram[at(array, slot)]);
            }
            body
        }

        pub(crate) fn project(&self, wram: &mut [u8; 8192], slot: usize) {
            let narrow = if slot < 16 { &NARROW[..] } else { &[] };
            for &array in WIDE.iter().chain(narrow) {
                wram[at(array, slot)] = self.byte(array);
            }
        }
    }

    impl Slots {
        pub(crate) fn lift(wram: &[u8; 8192]) -> Slots {
            let mut slots = Slots::default();
            for (i, slot) in slots.main.iter_mut().enumerate() {
                *slot = Slot {
                    kind: wram[at(0x7CC0, i)],
                    body: Body::lift(wram, i),
                    brain: crate::actor::families::lift(wram[at(0x7CC0, i)], wram, i),
                };
            }
            slots.spare = wram[at(super::SPARE, 0)..at(super::SPARE, 32)]
                .try_into()
                .expect("32 bytes");
            for (i, second) in slots.second.iter_mut().enumerate() {
                let kind = wram[at(0x7CC0, 24 + i)];
                *second = Second {
                    kind,
                    placed: None,
                    body: Body::lift(wram, 24 + i),
                    brain: crate::actor::families::Brain::new_second(kind),
                };
            }
            slots
        }

        pub(crate) fn project(&self, wram: &mut [u8; 8192]) {
            for (i, slot) in self.main.iter().enumerate() {
                wram[at(0x7CC0, i)] = slot.kind;
                slot.body.project(wram, i);
            }
            wram[at(super::SPARE, 0)..at(super::SPARE, 32)].copy_from_slice(&self.spare);
            for (i, second) in self.second.iter().enumerate() {
                wram[at(0x7CC0, 24 + i)] = second.kind;
                second.body.project(wram, 24 + i);
            }
        }
    }
}
