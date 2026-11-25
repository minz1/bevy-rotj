# Classic Mode and Enhanced Mode: how the port grows past 1.0 without losing parity

Status: architecture decision, 2026-10-15 (owner's direction), approved by a stand-in
reviewer with its changes applied (PLAN.md reconciled, input-only tapes for enhanced
baselines, interpolation's snap cases, the redundant "off is Classic" proof cut).
Applies from 1.1.0 (`docs/PLAN.md`: settings menu, FPS modes, widescreen) onward.

## Decision

The 1.0 port is the **Classic Mode** core: the game's loop, ported and verified byte for
byte against the 6502 oracle. After 1.0, every change to existing behaviour or
presentation is a **setting whose default is the NES's behaviour**; new content (bonus
levels, restorations) is added alongside. With every setting at its default the game is
Classic Mode, and the Classic suites (the oracle lockstep, the state tapes, random play
and exploration, picture parity) run with defaults only, at every merge and tag, for the
life of the project. **Enhanced Mode** is the game with settings changed; it is tested
per setting plus invariants, not per combination of settings.

The re-architecture after 1.0 (the "slim" refactor) happens under the Classic suites: a
refactor that changes one byte of Classic Mode fails the gates. That makes a large
restructure safe rather than a rewrite.

What Classic verifies is the game's state (RAM and WRAM at every frame) plus its
reference picture and sound, not how the app produces them. So the app's NES hardware
layers may be replaced piece by piece (the NES-style compositor by Bevy tilemaps and
sprites, say) as long as the Classic suites and Classic's picture and sound tests still
pass; new features are built the modern way, never as NES emulation. The 2A03 synth
stays as Classic's sound (small, finished, write-exact); remastered audio is an
Enhanced setting. The 6502 oracle is test tooling, never shipped, and stays while
Classic needs a ground truth.

What stays NES-exact in Classic forever: everything the lockstep compares, which is all
of RAM and WRAM, so the 1/16-pixel fixed point, the camera and one-hit deaths among it.
Changing any of them is an Enhanced setting with its own code path layered over the
Classic one (e.g. a separate physics path), which costs more than editing the Classic
code would; that cost is accepted as the price of keeping Classic verified.

## Two kinds of setting, two kinds of test

1. **Presentation** (the game's state is untouched): higher frame rates, CRT filters,
   volume, a modern render path. Higher frame rates keep the 60 Hz game tick and its
   fixed point; the renderer draws between the last two ticks by interpolating their
   integer positions, which adds one tick of display latency, and snaps (no
   interpolation) where motion is not continuous: teleports and respawns, room and
   scroll wraps, screen transitions, a sprite slot reused by another actor, the camera
   at a clamp. Tests: the Classic suites never see the setting; renderer tests that an
   interpolated frame lies between its two ticks except at the snap cases.
2. **Behaviour** (the game's state changes): widescreen (`World::view.width`, `256 *
   16` today, widens the spawn window), later gameplay options. Tests per setting:
   - its Enhanced path forced to the Classic value equals Classic (the Classic suites
     themselves cover the default path);
   - invariants with it on (no actor spawns inside the visible area; every room of the
     TAS route can still be completed; no slot overflows), checked by seeded random play
     and the exploration harness in that configuration;
   - **enhanced baselines**: input-only tapes recorded from the port in an approved
     configuration (the pads per tick, the settings, and an FNV-1a hash of RAM and WRAM
     every N ticks), replayed through the port alone and compared hash by hash.
     Re-recording one is a reviewed change. The existing state tape format stays
     oracle-only: it feeds the oracle's own writes into the port, which no longer fit a
     run that diverges from the game.

## Not decided here

The shape of the slim refactor, how the core receives settings (one `Settings` value
passed in is the likely shape), the settings UI, and how the Classic gates get the ROM
after the 1.0.1 "bring your own ROM" pass (a runner that has it). Saved setting names
are public interface (`docs/PLAN.md` "Versioning"); lag stays out of both modes (the
off-TAS spec's "Known gaps").
