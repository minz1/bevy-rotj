# The sprite attribute bits, named

Status: approved with changes by the Opus stand-in, 2026-10-18 (applied: the counts; one
`Player::faces_left` instead of a local copy; the bytes out of scope listed; each bit named by what
its reader does; `player.rs:461` left alone as a lead; the `facing` doc). The survey loop's first item (MODERNIZE "The survey loop, 2026-10-18",
#1). No behaviour change: constant substitution, proven by the lockstep.

## What is there

The OAM attribute byte's bits are written raw wherever a sprite's attributes are built or tested:
`0x40` flip x, `0x80` flip y, `0x20` behind the background, `0x03` the palette (and `0xFC` its
complement), on `work.attr`, `Body.flags` (`$7C00`), `Projectile.attr` and `Player.facing`
(`$046F`), plus the readers in `render.rs`, `oam.rs`, `ui.rs` and `modern.rs`: about 130 sites in
25 files. `player.rs` turns Batman with `facing ^= 0x40` 15 times and tests `facing & 0x40` 11
times (16 across the crate); `weapons.rs` already has a local `faces_left`.

## The change

- `oam::attr`, a module of named masks like 1.1.1's `pose`: `FLIP_X = 0x40`, `FLIP_Y = 0x80`,
  `BEHIND = 0x20`, `PALETTE = 0x03` (the names the app's `flip_x`/`flip_y` already use). Not a
  newtype: the bytes live in RAM and the game does arithmetic on them (`| call.attr`, `& 0xCF`).
- Each bit is named by what its reader does with it. `Body.flags` is the stored attribute byte
  (`work.attr |= body.flags`; `engine::store` writes it back): where a family reads its flip bit as
  its direction (`blast_walker`, `hover_shooter`), `FLIP_X`; where a family uses a bit as its own
  switch (`breakable_block`'s `flags & 0x80`), a family const, not `FLIP_Y`. Combined masks are
  spelled out (`0xC0` = `FLIP_X | FLIP_Y`, `0xFC` = `!PALETTE`); `0xCF` stays a literal.
- **Out of scope** (other meanings of the same bits): the local probe-result `flags` in
  `patrol_walker` and `crate_soldier`, `Projectile.flags` (spent, deflected), `probe_flags`,
  `death_flags` and `Probe.hit`.
- `Player.facing` keeps its name and its doc says what it is: Batman's sprite attribute byte
  (bit 6 faces left; the palette bits beside it, set where his palette changes).
  `weapons.rs`'s local `faces_left` moves to `Player::faces_left` and serves all 16 reads; the turn
  stays `facing ^= attr::FLIP_X`.
- `player.rs:461`'s `(toward ^ facing) & 0x80` is left as it is: `toward` never has bit 7 there,
  so it tests only `facing`'s bit 7; whether the ROM does the same is a lead of its own, since a
  fix would change behaviour.
- The readers (`render.rs`'s sprite line, `oam.rs`'s flip of `dx`, `ui.rs`, `modern.rs`) use the
  same names, so the Classic and Modern pictures decode the byte one way.

## Proof

The Classic suites, a sweep and a 300-branch exploration unchanged; tapes byte-identical; the
picture tests (`render.rs`, the app's pixels against the oracle's, the Modern render) unchanged.
