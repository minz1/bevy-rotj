# Batman: Return of the Joker — Rust/Bevy Port

A port of Sunsoft's 1991 NES game *Batman: Return of the Joker* to safe Rust and
[Bevy](https://bevyengine.org). It isn't an emulator. The game's logic was rebuilt
routine by routine from the disassembly, and a 6502 emulator in `tools/rotj-oracle`
checks the port byte for byte: the whole published TAS, from power-on through the
ending, matches the original's RAM and WRAM at every frame of its game loop. The 1.0 releases play like
the cartridge, sound and flicker included.

## You need your own ROM

None of the game is in this repository. The port reads it from your copy of
**Batman - Return of the Joker (USA).nes** at start-up (PRG+CHR MD5
`830111df30b8511562d4faca6aeb792d`; the header doesn't matter). Give it the ROM in
any of these ways:

- drop the `.nes` file onto `batman-rotj.exe`, or pass its path as the first argument;
- put it next to the executable;
- set `ROTJ_ROM=/path/to/rom.nes`;
- in a development checkout, put it in `raw/`.

The first time it finds one, it keeps a copy in your cache folder
(`%LOCALAPPDATA%\batman-rotj` on Windows, `~/.cache/batman-rotj` elsewhere), so later
launches need nothing. Without a ROM it says so and exits.

## Running

    cargo run --release -p batman-rotj

| NES        | Keyboard | Gamepad |
|------------|----------|---------|
| D-pad      | Arrows / WASD | D-pad |
| A (jump)   | X / K    | South |
| B (weapon) | Z / J    | West or East |
| Start      | Enter    | Start |
| Select     | Shift    | Select |

The window is resizable and the picture stays at an integer scale. If the renderer
misbehaves, `ROTJ_RENDER_BACKEND=gl` (or `vulkan`) picks a wgpu backend. `ROTJ_LEVEL=1..12`
starts on a later level; `ROTJ_RECORD_INPUT=<dir>` records your pads for a replay against
the original.

## What's here

| Path | What it is |
|------|------------|
| `crates/rotj-rom` | finds and checks your ROM |
| `crates/rotj-core` | the game: actors, levels, scripts, the boot and the ending, plus the NES-picture renderer |
| `crates/rotj-apu`, `crates/rotj-music` | the 2A03 synth and the game's own music driver, ported |
| `crates/app` | the Bevy app |
| `tools/rotj-oracle` | the 6502/FME-7/PPU emulator the tests compare against |
| `tools/rotj-tools` | the `rotj` extractor CLI (CHR, levels, music, actors) |
| `disasm/` | the disassembler: `disasm/assemble.sh` generates the listing from your ROM and rebuilds it byte for byte |
| `docs/` | the reverse-engineering notes and the plan; `docs/STATUS.md` is the current state |

## Tests

    cargo test --workspace                      # without a ROM, the ROM tests skip
    ROTJ_REQUIRE_ROM=1 cargo test --workspace   # with one, they must all run

The whole-game lockstep runs in release:
`ROTJ_REQUIRE_ROM=1 cargo test --release -p rotj-core -p rotj-oracle -- --ignored`.
Where a test was checked against the real emulator (fceux), the repository keeps only
a digest of that capture, so the comparison still holds without shipping the capture.

No `unsafe` anywhere (a workspace lint), clippy pedantic at zero warnings, and
`cargo deny` for licenses and advisories.

## License

The code is MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). *Batman: Return of the Joker* belongs
to its rights holders; this project ships none of it.

Classic Mode's colours are FirebrandX's
[Composite Direct](http://www.firebrandx.com/nespalette.html) palette
(`assets/graphics/palette-sources/ATTRIBUTION.md`). The whole-game input the tests replay
is Aglar's published TAS ([TASVideos 1087M](https://tasvideos.org/1087M), CC BY 2.0,
`tools/emu/tas/ATTRIBUTION.md`).
