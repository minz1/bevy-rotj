# The remaining flag bytes named

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: contact's bit 6 is
`PASSIVE` and bit 0 `COLLECTS`; the unread bits left unnamed on the ROM's evidence;
`contact::UNTARGETED` beside `STATE_UNTARGETED`, each linking the other; `probe_flags` bit 6 is
`BLOCKED_Y`; `Player.hit` included; `pose_step`'s flags are the slide's ledge probe; `death_flags`'
reads are `BIT`s and bit 0 is written but never read; `out_of_energy`, not `is_dead`; the anchor's
whole-byte tests kept; the proof's claimed-set step dropped; the family consts kept). Implemented in 1.1.30. The survey
loop's #9 (MODERNIZE "The survey loop, 2026-10-18"). No behaviour change: constant substitution,
proven by the lockstep.

## What is there

Seven bytes are tested and written with raw masks; each bit's meaning below comes from its readers
(and, for the unread bits, a scan of every absolute read of the address in the ROM):

- `Body.contact` (`$7EE0`, main slots), 9 reads in 4 files, about 23 writes in 17 files (plus the
  family consts `CHILD_CONTACT`, `PARTS_CONTACT`, `BURST_CONTACT`): `$80` shielded (shots bounce
  off), `$40` passive (touching never hurts; only an actor that also collects is touched, and then
  collected), `$20` not a homing target, `$10` hurt by the slide, `$08` proof against the Super's
  smite, `$01` collects (read only with `$40`, `$A9CA`). No ROM reader tests `$04` or `$02`.
  `diving_bomber` stores its kind into the byte, as the ROM stores the same A to both.
- `Work.probe_flags` (`$9F`), 16 sites: `$80` the tile dealt damage, `$40` the vertical probe met
  solid (the floor when falling, the ceiling when rising). `Player.hit` (`$A8`) receives it whole
  from `spikes` and is tested once, `$80`: spike damage lands even through invulnerability.
- `Work.anchor` (`$9A`): the port writes only 0 or `$40` (the bottom band), tested `& 0x40` 16 times;
  `breakable_block` tests the whole byte (`$8258`, `$8282`), and ROM writers store a computed A to it.
- `Player.pose_step` (`$047D`): a counter, and during the slide off a ledge two flags at 4 sites:
  `$80` the ledge probe has run, `$40` it found a floor.
- `Player.death_flags` (`$0465`), 11 sites: read only by two `BIT`s (bit 7: skip the dying overlay;
  hence `set_overflow_from`), bit 0 written by an `ORA #1` and never read.
- `energy & 0x80` (`$0478`), 8 sites: a sign test on a byte the game decrements; negative means out
  of energy (dead, or dying once the stun ends).
- `Scroll.mode` (`$C3`), 5 reads in `scroll.rs`, one write in `room.rs`: `$80` auto-scroll x, `$40`
  auto-scroll y, `$01` forward (`+$0A`: right in x, down in y). `$D2F3` and `$D343` read only those.

## The change

Mask modules beside each field, named by what the reader does, combinations spelled out:
- `slots::contact { SHIELDED = 0x80, PASSIVE = 0x40, UNTARGETED = 0x20, SLIDE_HURTS = 0x10,
  SMITE_PROOF = 0x08, COLLECTS = 0x01 }` (`0x49` = `PASSIVE | COLLECTS | SMITE_PROOF`, `0x48` =
  `PASSIVE | SMITE_PROOF`, `0x50` = `PASSIVE | SLIDE_HURTS`); `UNTARGETED`'s doc says that unlike
  `actors::STATE_UNTARGETED` (`$7D50` bit 6) it does not drop a homing lock already made, and each
  links the other. `CHILD_CONTACT`, `PARTS_CONTACT`, `BURST_CONTACT` stay, defined from the masks;
  `diving_bomber`'s `= CHILD` stays.
- `engine::probe_flags { DAMAGED = 0x80, BLOCKED_Y = 0x40 }`, also for `Player.hit`
  (`player.hit & probe_flags::DAMAGED`); the docs of `Work.probe_flags` and `Player.hit` say what
  the bits mean.
- `engine::anchor { BOTTOM = 0x40 }`; the field stays a `u8` (the lift and projection round-trip
  the byte); `breakable_block`'s `== 0` tests stay whole-byte tests, as the ROM's are.
- `world::slide_ledge { PROBED = 0x80, FLOOR_BELOW = 0x40 }` at `pose_step`'s 4 flag sites (`0xC0` =
  `PROBED | FLOOR_BELOW`); the counter uses stay; `pose_step`'s doc fixed.
- `world::death { NO_OVERLAY = 0x80, BURST = 0x01 }`, `BURST` documented as written but never read;
  the prose says the reads are `BIT`s.
- `Player::out_of_energy(&self) -> bool` (`#[must_use]`, beside `faces_left`):
  `self.energy.cast_signed() < 0`, at the 8 sites.
- `scroll::mode { AUTO_X = 0x80, AUTO_Y = 0x40, FORWARD = 0x01 }`, `FORWARD`'s doc giving the
  direction.
- Out of scope: `Body.hit` (`collision.rs`'s unconverted `hit::DYING` test goes to the survey list),
  `Player.moved`, the HUD's and palettes' `& 0x80` bytes, the auto-scroll speeds, the level packs'
  `contact` integer.

## Proof

Every address above is compared by the lockstep (none is in the oracle's `SCRATCH` list; `$9F` and
`$9A` at its peek brackets). The diff swaps masks for consts of the same value; afterwards a grep
finds no raw `0x80`/`0x40`/`0x20`/`0x10`/`0x08`/`0x01` against these fields. The Classic suites, a
sweep and a 300-branch exploration unchanged; tapes byte-identical.
