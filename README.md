# Batman: Return of the Joker — Rust/Bevy Port

A port of Sunsoft's 1991 NES game *Batman: Return of the Joker* to safe Rust and
[Bevy](https://bevyengine.org). It isn't an emulator. The game's logic was rebuilt
routine by routine from the disassembly, and a 6502 emulator in `tools/rotj-oracle`
checks the port byte for byte: the whole published TAS, from power-on through the
ending, matches the original's RAM and WRAM at every frame of its game loop. The 1.0 releases play like
the cartridge, sound and flicker included.

## You need your own ROM

None of the game is in this repository or in the downloads. You need your copy of
**Batman - Return of the Joker (USA).nes** (PRG+CHR MD5
`830111df30b8511562d4faca6aeb792d`; the header doesn't matter).

## Download

Each [release](https://github.com/minz1/bevy-rotj/releases) has a Windows exe and a
Linux one. Put your ROM next to the exe, or drop it onto the exe (on Linux, pass it as
the first argument); `ROTJ_ROM=/path/to/rom.nes` works too. The ROM stays where you put
it; the only file the game writes is `settings.ron`, next to the exe, when you close the
settings menu. If no ROM is found, the exe says where it looked.

## Build from source

    cargo run --release -p batman-rotj -- /path/to/rom.nes

That exe finds the ROM at start-up like the downloads. To build the ROM in instead, put
it in `raw/` or set `ROTJ_ROM=/absolute/path/to/rom.nes` before building: the build
checks it and compiles it in, so the exe runs on its own. That exe contains the game,
so keep it to yourself.

A ROM that is named but missing or wrong fails the build. With no ROM at all the
workspace still builds and tests (the ROM tests skip). If you add the ROM later, rebuild
(`touch crates/app/build.rs` if `raw/` didn't exist before).

## Playing

| NES        | Keyboard | Gamepad |
|------------|----------|---------|
| D-pad      | Arrows / WASD | D-pad |
| A (jump)   | X / K    | South |
| B (weapon) | Z / J    | West or East |
| Start      | Enter    | Start |
| Select     | Shift    | Select |
| Settings   | Esc / F1 | Guide, or click the right stick |

Esc opens the settings menu anywhere, and so does Select while the game is paused:
volume, overscan, palette, the sprite limit and vsync. Everything starts the way the NES
looked: a TV's overscan hides 8 lines at the top and bottom, the colours are Composite
Direct, and more than 8 sprites on a line flicker. Turn the sprite limit off and the
flicker from it goes. Pick FILE as the palette to use your own 64-colour `.pal`, saved
as `palette.pal` next to the exe.

The window is resizable and the picture stays at an integer scale. The game shows one
picture per 60 Hz tick, so on a variable-refresh display (G-Sync, FreeSync) every frame
lasts the same 1/60 s. If the renderer
misbehaves, `ROTJ_RENDER_BACKEND=gl` (or `vulkan`) picks a wgpu backend. `ROTJ_LEVEL=1..12`
starts on a later level; `ROTJ_RECORD_INPUT=<dir>` records your pads for a replay against
the original.

## What's here

| Path | What it is |
|------|------------|
| `crates/rotj-rom` | finds and checks your ROM (the app's build script embeds it, or the exe loads it at start-up) |
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
