# The actors' fields by name, not address (1.1.x, `docs/MODERNIZE.md` #13), slice 1

Status: approved with changes by the Opus stand-in, 2026-10-17 (applied: narrow fields at
slots 24-31; one alias mapping including `Age`, `load_aliases`/`store_aliases` on it;
`Kind` out of `Field`; the direct-write rule tightened; the address forms corrected; the
test against the WRAM layout; slice 2's `body.byte` callers). `docs/MODERNIZE.md` #13 ("address-keyed `byte()`/`set_byte()` matches as an
index enum"), the step the actors-as-entities decision ranks first whatever its outcome,
from a read-only survey of `slots.rs`, `world.rs` and their callers.

## What the survey found

- Actor fields are reached by WRAM address in four layers: `Body::byte`/`set_byte(array)`
  (30-arm matches on the array base), `Slots::byte`/`set_byte`, `slots::read`/`write`/
  `owned` over `find_owner` (the highest array base at or below an address: a wide array of
  32, a narrow one of 16, a projectile array, the kinds `$7CC0`, the spare bytes `$7F70`),
  and `Cx::byte`/`set_byte` on top.
- The aliasing is address arithmetic: a narrow array at slot 16-23 lands in the next
  array's slot 0-7, and `$7BA0 + 16` lands in the projectile array `$7BB0`.
  `load_aliases`/`store_aliases` mirror a running 16-23 slot's narrow fields that way.
  Hidden aliases too: the second-class platforms' bytes `HOME`/`LIMIT` (`$7BF0`),
  `PRESSED` (`$7F10`), `DIRECTION`, and `spawn::place`'s `$7BD8`/`$7EF8` at `24 + i` land
  in main slots 16-23's fields.
- 123 `cx.byte`/`cx.set_byte` calls (106 writes), nearly all a constant array base plus a
  slot (most of `final_boss`'s 16-23 writes fold the 16 into the constant: `$7B10 + y`,
  `$7CD0 + y`), with two cross-family reads (`next_area_gate` reads `$7BD0 + work.slot`, a
  projectile array indexed by an actor slot; `weapons.rs` reads `array + target`), and the
  platforms' `DIRECTION`, `HOME` and `LIMIT` all `$7BF0`, slots 16-23's `HealthLow`; about 95 write a child's fields right after spawning it (`final_boss` 39,
  `stage6_lobber` 13, `bobber_spawner` 12, ...), about 12 clear a slot's arrays, a handful
  write a parent's field, about 17 read (free-slot scans by kind, a weapon-kind guard).
  Plus `put(cx, base, slot, v)` wrappers (`ace_ranger`, `stage4_boss`, about 48 calls),
  about 59 `body.byte`/`set_byte` calls with an array constant, and the WRAM-image tooling
  (`lift`/`project`, `room.rs`'s image).
- Single bytes of 16-bit fields are written on their own (health low, vy low, x/y halves).

## Design (this slice)

- `actor::slots::Field`: the 30 body arrays, `#[repr(u16)]` with each array's base as its
  discriminant, named for the `Body` field and its half (`XHigh = 0x7B00`, `XLow`, `YHigh`,
  `YLow`, `Flags`, `ChrSlot`, `State`, `Behaviour`, `Subtype`, `Hit`, `Phase`, `Timer`,
  `Counter`, `Anim`, `Sprite`, `Cell`, `DyingTimer`, `Scratch`, `Contact`, `Age`,
  `HealthHigh`, `HealthLow`, `VxHigh`, `VxLow`, `VxDriftHigh`, `VxDriftLow`, `VyHigh`,
  `VyLow`, `RefVxHigh`, `RefVxLow`): `base()` (`self as u16`), `is_narrow()`, `ALL`, and
  `from_array(u16) -> Option<Field>` (`ALL.find`) for the ROM tables that name arrays.
  Byte-level halves, since arrays, ROM tables and the game's single-byte writes are all
  byte-level. The kinds are not a field: `Slots::kind(slot)`/`set_kind(slot, v)` for slots
  0-31 (`final_boss` clears `$7CD0 + 0..16`, the kinds of 16-31; the free-slot scans).
- `Body::get(Field) -> u8` and `Body::set(Field, u8)`, total over the 30 arrays, replace
  the address matches; `Body::byte`/`set_byte(array)` stay as thin wrappers (through
  `from_array`) until slice 3.
- One private mapping from (slot 0-31, `Field`) to a location, the only home of the
  aliasing: a wide field is `main[slot]` (0-23) or `second[slot - 24]`; a narrow field of
  slot 0-15 is `main[slot]`; a narrow field of slot 16-31 is the next array's slot
  `slot - 16` (0-15), which for `Age` is the projectile array `$7BB0` (`level_x_hi` of
  projectile `slot - 16`). `Slots::get`/`set` use it and panic on the projectile case (as
  `Slots::byte` does); `Cx::get`/`set` use it with the projectiles. `load_aliases`/
  `store_aliases` move onto it in this slice. The slot index is the game's own (the 6502's
  X/Y), so `base + 16 + y` becomes `(16 + y, Field::X)` mechanically.
- Pilot: `bobber_spawner` and `final_boss` move to `cx.get`/`cx.set(slot, Field::X)` and
  `Slots::kind`/`set_kind`, or to direct `Body` field writes where a whole field is written,
  nothing reads between its two bytes, and the field is wide or the slot is 0-15 (a slot
  16-23's narrow fields in `main[16+].body` are only a cache `load_aliases` refills, so a
  direct write there would be lost). Each site's write order is kept; it matters where a
  loop writes `kind` before the next `free_child_slot` scan (`spawn_be`, `spawn_c0`).

## Next slices

2. The other spawners and parent writes; the `put` wrappers; the platforms' hidden aliases
   as named accessors (their uses of `HealthLow` as a step or direction named for what
   they are); `weapons.rs`, `next_area_gate.rs`, `turret::Indexed`; the about 59
   `body.byte`/`set_byte` callers with an array constant (`floor_debris`, `hurtful_burst`,
   `spinner`, `turret`, the bursts and decorations, `ace_ranger_shot`, the platforms,
   `engine::move_by_velocity`/`spawn_parts` through `Field::from_array`).
3. `slots::read`/`write`/`owned`/`find_owner`, the `Body` address wrappers and
   `WIDE`/`NARROW` deleted once only the WRAM-image tooling uses addresses, which then
   iterates `Field::ALL`.

## Tests

- `fields_match_the_wram_layout`: a WRAM image filled with a pattern (two seeds), lifted
  into `Slots` and `Effects`; for every `Field` and slot 0-31, `Cx`'s `get` equals
  `wram[f.base() + slot]`, and a `set` projected back changes exactly that byte; the kinds
  likewise; `Field::from_array(f.base()) == Some(f)` for every field.
- The pilot families' oracle tests, the Classic suites, a 1-seed sweep and a 300-branch
  exploration; the tapes unchanged.

## Plan

1. Tests first; `Field`, `Body::get`/`set`, `Slots::get`/`set`, `Cx::get`/`set`.
2. The pilot; checks; the `reviewer` agent; commit `The actors' fields by name: the pilot`.
