//! Actor slots: the 24 main slots the actor loop runs, and the 8 second-class slots.

use crate::actor::families::{Brain, SecondBrain};
use crate::actor::world::{PROJECTILE_ARRAYS, Projectile};
use crate::actors::SpawnRecord;

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
    /// Hit flag (`$7D70`): see [`hit`].
    pub hit: u8,
    /// Handler phase (`$7DD0`).
    pub phase: u8,
    /// Handler timer (`$7DF0`).
    pub timer: u8,
    /// Handler counter (`$7E10`).
    pub counter: u8,
    /// Animation counter (`$7E90`).
    pub anim: u8,
    /// Metasprite frame the handler draws (`$7C30`).
    pub sprite: u8,
    /// Schedule cell it spawned from (`$7CE0`).
    pub cell: u8,
    /// Frames of the dying animation (`$7F00`).
    pub dying_timer: u8,
    /// A handler-local byte (`$7D30`); a second-class slot's ride width (`$CD5F`).
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

/// One of a slot's arrays, by the [`Body`] field (or half of it) it holds; its value is
/// the array's base address.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// `$7B00`: the high byte of [`Body::x`].
    XHigh = 0x7B00,
    /// `$7B20`: the low byte of [`Body::x`].
    XLow = 0x7B20,
    /// `$7B40`: the high byte of [`Body::y`].
    YHigh = 0x7B40,
    /// `$7B60`: the low byte of [`Body::y`].
    YLow = 0x7B60,
    /// `$7C00`: [`Body::flags`].
    Flags = 0x7C00,
    /// `$7C90`: [`Body::chr_slot`].
    ChrSlot = 0x7C90,
    /// `$7D50`: [`Body::state`].
    State = 0x7D50,
    /// `$7D90`: [`Body::behaviour`].
    Behaviour = 0x7D90,
    /// `$7DB0`: [`Body::subtype`].
    Subtype = 0x7DB0,
    /// `$7D70`: [`Body::hit`].
    Hit = 0x7D70,
    /// `$7DD0`: [`Body::phase`].
    Phase = 0x7DD0,
    /// `$7DF0`: [`Body::timer`].
    Timer = 0x7DF0,
    /// `$7E10`: [`Body::counter`].
    Counter = 0x7E10,
    /// `$7E90`: [`Body::anim`].
    Anim = 0x7E90,
    /// `$7C30`: [`Body::sprite`].
    Sprite = 0x7C30,
    /// `$7CE0`: [`Body::cell`].
    Cell = 0x7CE0,
    /// `$7F00`: [`Body::dying_timer`].
    DyingTimer = 0x7F00,
    /// `$7D30`: [`Body::scratch`].
    Scratch = 0x7D30,
    /// `$7EE0`: [`Body::contact`].
    Contact = 0x7EE0,
    /// `$7BA0`: [`Body::age`].
    Age = 0x7BA0,
    /// `$7EF0`: the high byte of [`Body::health`].
    HealthHigh = 0x7EF0,
    /// `$7BE0`: the low byte of [`Body::health`].
    HealthLow = 0x7BE0,
    /// `$7E30`: the high byte of [`Body::vx`].
    VxHigh = 0x7E30,
    /// `$7E40`: the low byte of [`Body::vx`].
    VxLow = 0x7E40,
    /// `$7E50`: the high byte of [`Body::vx_drift`].
    VxDriftHigh = 0x7E50,
    /// `$7E60`: the low byte of [`Body::vx_drift`].
    VxDriftLow = 0x7E60,
    /// `$7E70`: the high byte of [`Body::vy`].
    VyHigh = 0x7E70,
    /// `$7E80`: the low byte of [`Body::vy`].
    VyLow = 0x7E80,
    /// `$7ED0`: the high byte of [`Body::ref_vx`].
    RefVxHigh = 0x7ED0,
    /// `$7EC0`: the low byte of [`Body::ref_vx`].
    RefVxLow = 0x7EC0,
}

impl Field {
    /// Every array, in [`Body`]'s order.
    pub const ALL: [Field; 30] = [
        Field::XHigh,
        Field::XLow,
        Field::YHigh,
        Field::YLow,
        Field::Flags,
        Field::ChrSlot,
        Field::State,
        Field::Behaviour,
        Field::Subtype,
        Field::Hit,
        Field::Phase,
        Field::Timer,
        Field::Counter,
        Field::Anim,
        Field::Sprite,
        Field::Cell,
        Field::DyingTimer,
        Field::Scratch,
        Field::Contact,
        Field::Age,
        Field::HealthHigh,
        Field::HealthLow,
        Field::VxHigh,
        Field::VxLow,
        Field::VxDriftHigh,
        Field::VxDriftLow,
        Field::VyHigh,
        Field::VyLow,
        Field::RefVxHigh,
        Field::RefVxLow,
    ];

    /// The array's base address.
    #[must_use]
    pub const fn base(self) -> u16 {
        self as u16
    }

    /// Kept by slots 0-15 only (16 wide); slots 16-31 index the next array.
    #[must_use]
    pub const fn is_narrow(self) -> bool {
        matches!(
            self,
            Field::Contact
                | Field::Age
                | Field::HealthHigh
                | Field::VxHigh
                | Field::VxLow
                | Field::VxDriftHigh
                | Field::VxDriftLow
                | Field::VyHigh
                | Field::VyLow
                | Field::RefVxHigh
                | Field::RefVxLow
        )
    }

    /// The field whose array starts at `array`, if one does.
    #[must_use]
    pub fn from_array(array: u16) -> Option<Field> {
        Field::ALL.into_iter().find(|field| field.base() == array)
    }
}

/// Where a slot's field lives: a body, or (slot 16-31's [`Field::Age`]) a projectile array.
enum Location {
    Main(usize, Field),
    Second(usize, Field),
    Projectile(usize, u16),
}

/// Where field `field` of slot `slot` (0-31, the game's index) lives: a narrow field of
/// slot 16-31 is the next array's slot `slot - 16`.
fn locate(slot: usize, field: Field) -> Location {
    if field.is_narrow() && slot >= 16 {
        let next = field.base() + 16;
        return match Field::from_array(next) {
            Some(next) => locate(slot - 16, next),
            None => Location::Projectile(slot - 16, next),
        };
    }
    if slot < 24 {
        Location::Main(slot, field)
    } else {
        Location::Second(slot - 24, field)
    }
}

/// Field `field` of slot `slot` (0-31), wherever the game's layout puts it.
#[must_use]
pub fn get(slots: &Slots, projectiles: &[Projectile; 16], slot: usize, field: Field) -> u8 {
    match locate(slot, field) {
        Location::Projectile(index, array) => projectiles[index].byte(array),
        _ => slots.get(slot, field),
    }
}

/// Writes field `field` of slot `slot` (0-31), wherever the game's layout puts it.
pub fn set(
    slots: &mut Slots,
    projectiles: &mut [Projectile; 16],
    slot: usize,
    field: Field,
    value: u8,
) {
    match locate(slot, field) {
        Location::Projectile(index, array) => projectiles[index].set_byte(array, value),
        _ => slots.set(slot, field, value),
    }
}

impl Slots {
    /// Field `field` of slot `slot` (0-31); panics where it is a projectile's (see [`get`]).
    #[must_use]
    pub fn get(&self, slot: usize, field: Field) -> u8 {
        match locate(slot, field) {
            Location::Main(slot, field) => self.main[slot].body.get(field),
            Location::Second(slot, field) => self.second[slot].body.get(field),
            Location::Projectile(..) => panic!("slot {slot}'s {field:?} is a weapon array"),
        }
    }

    /// Writes field `field` of slot `slot` (0-31); panics where it is a projectile's.
    pub fn set(&mut self, slot: usize, field: Field, value: u8) {
        match locate(slot, field) {
            Location::Main(slot, field) => self.main[slot].body.set(field, value),
            Location::Second(slot, field) => self.second[slot].body.set(field, value),
            Location::Projectile(..) => panic!("slot {slot}'s {field:?} is a weapon array"),
        }
    }

    /// Second-class slot `second`'s parameter, which the game keeps in main slot
    /// `16 + second`'s [`Field::HealthLow`] (`$7BD8 + 24 + second`, the spawn record's
    /// health high byte): a sinking platform's home row, a shuttle's travel limit, a path
    /// platform's direction.
    #[must_use]
    pub fn platform_parameter(&self, second: usize) -> u8 {
        self.get(16 + second, Field::HealthLow)
    }

    /// Sets second-class slot `second`'s parameter ([`Slots::platform_parameter`]).
    pub fn set_platform_parameter(&mut self, second: usize, value: u8) {
        self.set(16 + second, Field::HealthLow, value);
    }

    /// The player's weight on second-class slot `second` (`$7EF8 + 24 + second`, main slot
    /// `16 + second`'s [`Field::DyingTimer`]): bit 7 while he stands on it.
    #[must_use]
    pub fn pressed(&self, second: usize) -> u8 {
        self.get(16 + second, Field::DyingTimer)
    }

    /// Sets the player's weight on second-class slot `second` ([`Slots::pressed`]).
    pub fn set_pressed(&mut self, second: usize, value: u8) {
        self.set(16 + second, Field::DyingTimer, value);
    }

    /// Slot `slot`'s occupant type (`$7CC0`, slots 0-31).
    #[must_use]
    pub fn kind(&self, slot: usize) -> u8 {
        if slot < 24 {
            self.main[slot].kind
        } else {
            self.second[slot - 24].kind
        }
    }

    /// Sets slot `slot`'s occupant type (`$7CC0`, slots 0-31).
    pub fn set_kind(&mut self, slot: usize, kind: u8) {
        if slot < 24 {
            self.main[slot].kind = kind;
        } else {
            self.second[slot - 24].kind = kind;
        }
    }
}

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
        for field in Field::ALL.into_iter().filter(|field| field.is_narrow()) {
            body.set(field, get(slots, projectiles, index, field));
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
        for field in Field::ALL.into_iter().filter(|field| field.is_narrow()) {
            set(slots, projectiles, index, field, body.get(field));
        }
    }
}

fn hi(value: u16) -> u8 {
    value.to_be_bytes()[0]
}

fn lo(value: u16) -> u8 {
    value.to_be_bytes()[1]
}

impl Body {
    /// The slot's byte in `field`'s array.
    #[must_use]
    pub fn get(&self, field: Field) -> u8 {
        match field {
            Field::XHigh => hi(self.x),
            Field::XLow => lo(self.x),
            Field::YHigh => hi(self.y),
            Field::YLow => lo(self.y),
            Field::Flags => self.flags,
            Field::ChrSlot => self.chr_slot,
            Field::State => self.state,
            Field::Behaviour => self.behaviour,
            Field::Subtype => self.subtype,
            Field::Hit => self.hit,
            Field::Phase => self.phase,
            Field::Timer => self.timer,
            Field::Counter => self.counter,
            Field::Anim => self.anim,
            Field::Sprite => self.sprite,
            Field::Cell => self.cell,
            Field::DyingTimer => self.dying_timer,
            Field::Scratch => self.scratch,
            Field::Contact => self.contact,
            Field::Age => self.age,
            Field::HealthHigh => hi(self.health),
            Field::HealthLow => lo(self.health),
            Field::VxHigh => hi(self.vx.cast_unsigned()),
            Field::VxLow => lo(self.vx.cast_unsigned()),
            Field::VxDriftHigh => hi(self.vx_drift.cast_unsigned()),
            Field::VxDriftLow => lo(self.vx_drift.cast_unsigned()),
            Field::VyHigh => hi(self.vy.cast_unsigned()),
            Field::VyLow => lo(self.vy.cast_unsigned()),
            Field::RefVxHigh => hi(self.ref_vx.cast_unsigned()),
            Field::RefVxLow => lo(self.ref_vx.cast_unsigned()),
        }
    }

    /// Writes the slot's byte in `field`'s array.
    pub fn set(&mut self, field: Field, value: u8) {
        let set_hi = |word: u16| u16::from_be_bytes([value, lo(word)]);
        let set_lo = |word: u16| u16::from_be_bytes([hi(word), value]);
        let set_hi_i = |word: i16| set_hi(word.cast_unsigned()).cast_signed();
        let set_lo_i = |word: i16| set_lo(word.cast_unsigned()).cast_signed();
        match field {
            Field::XHigh => self.x = set_hi(self.x),
            Field::XLow => self.x = set_lo(self.x),
            Field::YHigh => self.y = set_hi(self.y),
            Field::YLow => self.y = set_lo(self.y),
            Field::Flags => self.flags = value,
            Field::ChrSlot => self.chr_slot = value,
            Field::State => self.state = value,
            Field::Behaviour => self.behaviour = value,
            Field::Subtype => self.subtype = value,
            Field::Hit => self.hit = value,
            Field::Phase => self.phase = value,
            Field::Timer => self.timer = value,
            Field::Counter => self.counter = value,
            Field::Anim => self.anim = value,
            Field::Sprite => self.sprite = value,
            Field::Cell => self.cell = value,
            Field::DyingTimer => self.dying_timer = value,
            Field::Scratch => self.scratch = value,
            Field::Contact => self.contact = value,
            Field::Age => self.age = value,
            Field::HealthHigh => self.health = set_hi(self.health),
            Field::HealthLow => self.health = set_lo(self.health),
            Field::VxHigh => self.vx = set_hi_i(self.vx),
            Field::VxLow => self.vx = set_lo_i(self.vx),
            Field::VxDriftHigh => self.vx_drift = set_hi_i(self.vx_drift),
            Field::VxDriftLow => self.vx_drift = set_lo_i(self.vx_drift),
            Field::VyHigh => self.vy = set_hi_i(self.vy),
            Field::VyLow => self.vy = set_lo_i(self.vy),
            Field::RefVxHigh => self.ref_vx = set_hi_i(self.ref_vx),
            Field::RefVxLow => self.ref_vx = set_lo_i(self.ref_vx),
        }
    }

    /// The slot's byte in `array` (one of [`WIDE`] or [`NARROW`]).
    #[must_use]
    pub fn byte(&self, array: u16) -> u8 {
        self.get(
            Field::from_array(array).unwrap_or_else(|| panic!("${array:04X} is not a Body array")),
        )
    }

    /// Writes the slot's byte in `array`, as `$AC16` writes parts generically.
    pub fn set_byte(&mut self, array: u16, value: u8) {
        let field =
            Field::from_array(array).unwrap_or_else(|| panic!("${array:04X} is not a Body array"));
        self.set(field, value);
    }
}

impl Body {
    /// Writes a spawn record as the spawners do for slots 0-15; other fields keep residue.
    pub fn spawn(&mut self, record: &SpawnRecord, cell: u8) {
        self.x = record.x;
        self.y = record.y;
        self.flags = record.flags;
        self.chr_slot = record.chr_slot;
        self.state = record.state;
        self.behaviour = record.behaviour;
        self.subtype = record.subtype;
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
        self.health = record.health;
        self.contact = record.contact;
        self.cell = cell;
    }
}

/// The bits of [`Body::hit`] (`$7D70`) the engine sets and tests.
pub mod hit {
    /// The actor is dying (`$AB13` sets it; `$AF97` and the families test it).
    pub const DYING: u8 = 0x80;
    /// The actor flashes from a hit; the low bits count the flash down.
    pub const FLASHING: u8 = 0x40;
    /// A fresh hit: flashing, two frames to count.
    pub const STRUCK: u8 = FLASHING | 2;
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

/// A second-class slot (24-31), run by `$CE7F` over the 32-wide arrays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Second {
    /// Occupant type (`$7CD8`), 0 when free.
    pub kind: u8,
    /// Its 32-wide arrays.
    pub body: Body,
    /// Its family's state.
    pub brain: SecondBrain,
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
    use super::{Body, NARROW, Second, SecondBrain, Slot, Slots, WIDE};

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
                    body: Body::lift(wram, 24 + i),
                    brain: SecondBrain::new(kind),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::world::Effects;

    /// The fields are the arrays `WIDE` and `NARROW` list, narrow exactly where they say.
    #[test]
    fn fields_are_the_slot_arrays() {
        let mut arrays: Vec<u16> = WIDE.into_iter().chain(NARROW).collect();
        arrays.sort_unstable();
        let mut fields: Vec<u16> = Field::ALL.map(Field::base).to_vec();
        fields.sort_unstable();
        assert_eq!(fields, arrays);
        for field in Field::ALL {
            assert_eq!(
                field.is_narrow(),
                NARROW.contains(&field.base()),
                "{field:?}"
            );
        }
    }

    /// Every field of every slot reads and writes the WRAM byte the game's layout puts it
    /// at (`base + slot`), through the aliases and the second-class slots.
    #[test]
    fn fields_match_the_wram_layout() {
        for seed in [0x35u8, 0xC6] {
            let mut wram = [0u8; 8192];
            for (i, byte) in wram.iter_mut().enumerate() {
                *byte = u8::try_from((i * 7 + i / 251) % 256).expect("a byte") ^ seed;
            }
            let ram = [0u8; 2048];
            let mut slots = Slots::lift(&wram);
            let mut effects = Effects::lift(&ram, &wram);
            for field in Field::ALL {
                assert_eq!(Field::from_array(field.base()), Some(field));
                for slot in 0..32 {
                    let at = usize::from(field.base() - 0x6000) + slot;
                    assert_eq!(
                        get(&slots, &effects.projectiles, slot, field),
                        wram[at],
                        "{field:?} of slot {slot}"
                    );
                    let value = !wram[at];
                    set(&mut slots, &mut effects.projectiles, slot, field, value);
                    let (mut slots_image, mut effects_image) = (wram, wram);
                    slots.project(&mut slots_image);
                    let mut ram_image = ram;
                    effects.project(&mut ram_image, &mut effects_image);
                    let changed = |image: &[u8; 8192]| {
                        (0..8192)
                            .filter(|&i| image[i] != wram[i])
                            .collect::<Vec<_>>()
                    };
                    let mut moved = changed(&slots_image);
                    moved.extend(changed(&effects_image));
                    moved.dedup();
                    assert_eq!(moved, [at], "{field:?} of slot {slot}");
                    set(&mut slots, &mut effects.projectiles, slot, field, wram[at]);
                }
            }
            for slot in 0..32 {
                let at = usize::from(KINDS - 0x6000) + slot;
                assert_eq!(slots.kind(slot), wram[at]);
                let mut moved = slots.clone();
                moved.set_kind(slot, !wram[at]);
                let mut image = wram;
                moved.project(&mut image);
                let changed: Vec<usize> = (0..8192).filter(|&i| image[i] != wram[i]).collect();
                assert_eq!(changed, [at], "the kind of slot {slot}");
            }
        }
    }
}
