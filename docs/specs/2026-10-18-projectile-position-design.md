# The projectile's position as words

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: `byte`/`set_byte`; the
full inventory; the homing target kept by address through `Cx::byte`, since a stale target can't be
ruled out; no new helper; the `$FF` reset pinned by a test first and written as a branch). The
survey loop's #5 (MODERNIZE "The survey loop, 2026-10-18"). No behaviour change: the weapons'
oracle tests and the lockstep prove it.

## What is there

- `Projectile` keeps its level position as four bytes, `level_x_hi` (`$7BB0`), `level_x_lo`
  (`$7BC0`), `level_y_hi` (`$7C60`), `level_y_lo` (`$7C70`): 75 lines (weapons.rs 51, world.rs 12,
  player.rs 8, stage.rs 4) plus the app's `mirror.rs` (4). Most sites pack and unpack them as a
  word; `advance` (weapons.rs) adds the velocity byte-wise with carry; four sites copy all four
  bytes from a source shot; `three_way_pull` and the edge tests read the high byte's bit 7; the
  orbit (weapons.rs, `hi & 0x0F`) reads the high byte's low nibble; a test writes the high byte
  alone; `stage.rs` builds one from a literal.
- `level_x_hi`'s array `$7BB0` is also slots 16-23's `$7BA0` (`Body::age`, the only narrow field
  that lands in a projectile array); `slots::locate` routes it, and `Projectile::byte`/`set_byte`
  serve it and the oracle's lift and projection by address.
- The homing shot reads its target's arrays by address through a local `actor` closure
  (`slots::read(.., array + target)`: the kind `$7CC0`, the state `$7D50`, the position
  `$7B00`/`$7B20`, `$7B40`/`$7B60`), duplicating `Cx::byte`. The target is 0-15 or `$FF` in every
  path traced, but one (the death path switching Super back without freeing the slots) can leave a
  stale `param`, so the reads stay by address.
- The reset writes `0x7D00 + reset`: `reset` is the shot's slot (its x velocity's high byte), or
  `$FF` when `homing_spawn_circlers` fails, which frees the slots with `LDX #$0F .. DEX; BPL` and
  returns with X = `$FF`, so the ROM's `STA $7D00,X` at `$84E3` writes `$7DFF`: main slot 15's
  `Body::timer` (`$7DF0 + 15`). The lockstep almost certainly never reaches that path (it needs
  14 or more charged slots).

## The change

- First, a unit test where `homing_spawn_circlers` fails, asserting slot 15's timer is cleared,
  run before and after the change; and `seen(0x8617)` on its `return false` path.
- `Projectile::x: u16` and `Projectile::y: u16` (level position, 1/16 px, as `Player` and `Body`
  name theirs beside `screen_x`/`screen_y`) replace the four bytes; their docs give the addresses
  and the `$7BA0` alias. `byte`/`set_byte` map `$7BB0`/`$7BC0`/`$7C60`/`$7C70` to the words'
  halves, so the alias, the lift and the projection are unchanged. The copies become
  `(x, y) = (source.x, source.y)`; `advance` becomes `x.wrapping_add(vx.cast_unsigned())`; the
  bit-7 tests `& 0x8000 != 0`; the orbit's `let hi = x.to_be_bytes()[0]`; the test keeps the low
  byte when it sets the high; `mirror.rs` reads the words.
- The homing shot's `actor` closure becomes `cx.byte(array + u16::from(target))`, still by
  address.
- The reset is a branch: on failure `cx.set(15, Field::Timer, 0)` with a one-line comment that it
  is the ROM's `STA $7D00,X` with X = `$FF`; otherwise `vx`'s high byte cleared, its low byte kept.
- Out of scope: `screen_x`/`screen_y`, `vx`/`vy` (already words), `Projectile`'s state bits (the
  survey's #6), the stale-target lead (logged in MODERNIZE: whether the ROM steers a Super shot
  left live by the death path).

## Proof

The new reset test before and after; the weapons' oracle tests (each weapon's flight compared with
the ROM's RAM frame by frame), the Classic suites, a sweep and a 300-branch exploration unchanged;
tapes byte-identical.
