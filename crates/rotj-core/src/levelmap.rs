//! The sublevel's nametable: the metatile map and the tables that tile it.
//!
//! The visible nametable is not stored in ROM; the game builds it at run time
//! from a **metatile map** and four **auto-tile tables**. Everything here was
//! verified against fceux's stage 1-1 captures and the emulator dumps, which the tests
//! now reproduce with the oracle and pin by digest (`docs/LEVELDATA.md` records the
//! container format and the corrections).
//!
//! # The map (`$6000-$6EFF`)
//!
//! WRAM holds 15 pages of 256 bytes at `$6000`, `$6100`, ... `$6E00`. The map is
//! **column-major**: page `r` is metatile row `r`, and byte `c` of each page is
//! metatile column `c`. So the sublevel is [`MAP_ROWS`] x [`MAP_COLUMNS`]
//! metatiles, i.e. 4096 x 240 px, and the whole map is resident from the moment
//! the sublevel loads (it is byte-identical at frames 600-3000 of
//! `tools/emu/walkall/`; it is *not* streamed in a window).
//!
//! # The tables (`$7000`, `$70D0`, `$71A0`, `$7270`)
//!
//! Each is 256 bytes, indexed by metatile id, and gives one corner of the 2x2
//! nametable block the metatile expands to: top-left, top-right, bottom-left,
//! bottom-right respectively (bank 0D `sub_B351`, `docs/LEVELDATA.md` §2). The
//! tables are generated per area from a compact CHR-resident definition by bank
//! 02 `$8B9E`, decoded here from the ROM ([`auto_tile_tables`]).
//!
//! # The scroll
//!
//! The PPU is set to single-screen mirroring (both nametables alias, so a dump
//! shows them identical), so the game scrolls with a 32-tile-column **ring**:
//! nametable address `p` holds the level tile column `c` with `c ≡ p (mod 32)`
//! and `c` inside the window `[left, left + 31]`, where `left = scroll_x / 8`.
//! This reproduces the captured nametables **byte-for-byte** at every captured
//! camera position (see the acceptance tests).

use thiserror::Error;

/// Metatile rows in the map (`$6000-$6EFF`), one 256-byte page each.
pub const MAP_ROWS: usize = 15;
/// Metatile columns in the map, one byte per page.
pub const MAP_COLUMNS: usize = 256;
/// One physical nametable (`$2000-$23FF`).
pub const NAMETABLE_BYTES: usize = 1024;
/// Tiles per nametable row (and the ring length in tiles).
pub const NAMETABLE_COLUMNS: usize = 32;
/// Metatile edge in NES pixels.
pub const METATILE_PIXELS: usize = 16;
/// The sublevel's full width: every metatile column, i.e. 4096 px.
pub const LEVEL_WIDTH_PIXELS: usize = MAP_COLUMNS * METATILE_PIXELS;
/// CPU/WRAM address of the map's first byte.
pub const MAP_ADDR: u16 = 0x6000;
/// Addresses of the four auto-tile tables: top-left, top-right, bottom-left,
/// bottom-right (bank 0D `sub_B351`).
pub const TILE_TABLE_ADDRS: [u16; 4] = [0x7000, 0x70D0, 0x71A0, 0x7270];
/// Bytes per auto-tile table (one per metatile id).
pub const TILE_TABLE_BYTES: usize = 256;

/// Metatile rows the ROM map decoder produces (`$6000-$6FFF`): 16 pages. The
/// renderer only reads rows 0-14 ([`MAP_ROWS`]).
pub const ROM_MAP_ROWS: usize = 16;

/// First CHR bank of the level *payload* (`$F8`): bank 02 `$99A9` maps banks
/// `$F8-$FF` to PPU `$0000-$1FFF` and copies them to WRAM `$6000-$7E5F`.
pub const PAYLOAD_FIRST_BANK: u8 = 0xF8;
/// First CHR bank of the intro and title's payload (`$F0`), copied to WRAM `$6000` at boot.
pub const BOOT_FIRST_BANK: u8 = 0xF0;
/// CHR bank supplying the payload's last `$1A0` bytes (`$7E60-$7FFF`), via
/// bank 02 `$9925`: bank `$F7`, offset `$260` (`$1E60` in the PPU window).
pub const PAYLOAD_TAIL_BANK: u8 = 0xF7;
/// Offset in [`PAYLOAD_TAIL_BANK`] of the payload tail.
pub const PAYLOAD_TAIL_OFFSET: u16 = 0x260;
/// Length of the payload tail (`$7E60-$7FFF`).
pub const PAYLOAD_TAIL_BYTES: usize = 0x1A0;

/// Areas the two bank-02 per-area tables describe (`$0400 / 2`).
pub const AREA_COUNT: usize = 18;

/// Bank 02 offset `$0B63` = CPU `$8B63`: the CHR 1 KiB base bank per area, read
/// by the map loader `$89FE` at `$8B63 + $0400/2`. It runs to `$8B74` inclusive
/// (index 17 = `$EA`), immediately before the PPU table.
///
/// The same table serves every stage: there is exactly one copy of it in the
/// ROM, and the loader always reads it. A level's CHR set is therefore selected
/// only by the area index `$0400 / 2`, which the game sets from bank 14's
/// [`LEVEL_AREA_TABLE`].
const AREA_BANK: usize = 0x8B63;

/// Bank 02 `$8B75`/`$8B76` = CPU `$8B75`/`$8B76`: the PPU address the area's map
/// stream is read from, one 16-bit little-endian entry per area (`$8B75-$8B98`),
/// matching [`AREA_BANK`].
const AREA_PPU: usize = 0x8B75;

fn byte(prg: &[u8], bank: usize, addr: usize) -> Option<u8> {
    prg.get(bank.checked_mul(0x2000)? + (addr & 0x1FFF))
        .copied()
}

fn word(prg: &[u8], bank: usize, addr: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        byte(prg, bank, addr)?,
        byte(prg, bank, addr + 1)?,
    ]))
}

/// Bank 02 `$8B63[$0400/2]`: the CHR 1 KiB base bank the loader maps for `area`.
#[must_use]
pub fn area_bank(prg: &[u8], area: usize) -> Option<u8> {
    if area >= AREA_COUNT {
        return None;
    }
    byte(prg, 2, AREA_BANK + area)
}

/// Bank 02 `$8B75[$0400/2]`: the PPU address the area's map stream is read from.
#[must_use]
pub fn area_ppu(prg: &[u8], area: usize) -> Option<u16> {
    if area >= AREA_COUNT {
        return None;
    }
    word(prg, 2, AREA_PPU + area * 2)
}

/// Where area `area`'s auto-tiler definition sits in the CHR: its 1 KiB bank
/// (bank 02 `$8ECF`) times 1024 plus its PPU address (bank 02 `$8EE1`).
fn class_start(prg: &[u8], area: usize) -> Option<usize> {
    if area >= AREA_COUNT {
        return None;
    }
    let bank = byte(prg, 2, CLASS_BANK + area)?;
    let ppu = word(prg, 2, CLASS_PPU + area * 2)?;
    Some(usize::from(bank) * 1024 + usize::from(ppu))
}

/// Bank 14 offset `$194E` = CPU `$D94E`: the level index (`$20`) -> `$0400`
/// value, read at level start by `bank0E:sub_D3D8` (`LDX $20; LDA $D94E,x;
/// STA $0400`). `$0400 / 2` is the index into bank 02's `$8B63` /
/// `$8B75` per-area tables, so this is the level -> CHR-set table. It lists 13
/// levels; `$0400` values are even (area = value / 2). Stage 1-1 is level 0 ->
/// `$0400 = 0` -> area 0.
///
/// **Verified: every row selects its area this way.** Forcing `$20 = n` at CPU
/// `$D3D8` (`tools/emu/stagearea-probe/probe.lua`; the game's own next instruction
/// `LDX $20` reads it) makes `sub_D3D8` write `$0400 = $D94E[n]` — traced at PC
/// `$D3E0` for every `n` in 0..12 — and the game then shows its own
/// `STAGE <n/2 + 1>-<n%2 + 1>` banner on the transition screen and loads area
/// `n / 2`'s map. See `docs/LEVELDATA.md` §13e for the per-row captures.
///
/// **Not yet observed**: the game *advancing* `$20` on its own during play (the
/// table is entered once per power-on/reset path); no captured run leaves level
/// 0. The counter semantics above the first level remain a lead.
pub const LEVEL_AREA_TABLE: [u8; 13] = [
    0x00, 0x02, 0x06, 0x08, 0x0A, 0x0C, 0x10, 0x12, 0x16, 0x18, 0x1A, 0x1C, 0x20,
];

/// Bytes in one nametable's attribute table (`$23C0-$23FF`): 8x8 quads.
pub const ATTRIBUTE_BYTES: usize = 64;
/// 256 px pages in the map's width, i.e. attribute tables in WRAM `$7600-$79FF`.
pub const ATTRIBUTE_PAGES: usize = MAP_COLUMNS / 16;

/// Bank 02 `$8ECF` = CPU `$8ECF`: the CHR 1 KiB base bank the **auto-tiler**
/// (`$8B9E`) reads its area definition from, one entry per area (`$0400 / 2`).
/// This is a *different* table from [`AREA_BANK`], which the map stream uses;
/// both describe the same area but sit in different CHR banks.
const CLASS_BANK: usize = 0x8ECF;

/// Bank 02 `$8EE1`/`$8EE2`: the PPU address of the area definition the
/// auto-tiler reads through `$2007` (the map loader's equivalent is
/// [`AREA_PPU`]).
const CLASS_PPU: usize = 0x8EE1;

/// `LDA $2007` count of each auto-tiler handler (jump table bank 02 `$8C1D`,
/// 19 entries): shape `s` consumes this many definition bytes per metatile.
const HANDLER_READS: [u8; 19] = [1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 4];

/// Decode the metatile map (`$6000-$6FFF`, [`ROM_MAP_ROWS`] x [`MAP_COLUMNS`])
/// for one area straight from the ROM's CHR, exactly as the game's loader does.
///
/// The loader is bank 02 `$89FE` (called from bank 13 `$BD5E`). It:
///
/// 1. maps CHR 1 KiB banks `$8B63[area]..+7` to PPU `$0000-$1FFF`
///    and points `$2006` at `$8B75[area]`;
/// 2. zeroes WRAM `$6000-$6FFF`;
/// 3. runs an LZ-style bit stream (header `count = (b0<<8 | b1) - 4`; 8 LSB-first
///    bits per byte; bit 1 = literal byte, bit 0 = copy: `lo`, `b2` give source
///    `$6000 + ((b2>>4)<<8) + lo` and length `(b2 & $0F) + 3`, copying from and
///    into the same 4 KiB window), emitting to `$7000-$7FFF`;
/// 4. transposes `$7000-$7FFF` into `$6000-$6FFF` as 16x16-byte blocks:
///    `out[k*256+p*16+j] = in[p*256+k*16+j]`.
///
/// `prg` is the cartridge's PRG-ROM; `chr` its CHR-ROM. `area` indexes the two
/// per-area tables (bank 02 `$8B63`/`$8B75`). The part of `$7000-$7FFF` the stream
/// does not reach still holds the level *payload* (banks `$F8-$FF`), which the
/// transpose copies into map columns `160..255`; this reproduces fceux's captured
/// map byte-for-byte, including those (uninitialised) columns.
///
/// Returns `None` when `prg`/`chr` are too short or `area` is out of range.
#[must_use]
pub fn map_from_rom(
    prg: &[u8],
    chr: &[u8],
    area: usize,
) -> Option<[[u8; MAP_COLUMNS]; ROM_MAP_ROWS]> {
    let mut image = payload_image(chr)?;
    map_over(prg, chr, area, &mut image)
}

/// `$7000-$7FFF` as the payload leaves them: CHR bank `$F8` offset `$1000` (bank
/// `$FC`), then the tail from bank `$F7` offset `$260`; `$7FC0-$7FFF` zero.
#[must_use]
pub fn payload_image(chr: &[u8]) -> Option<[u8; 0x1000]> {
    let mut image = [0u8; 0x1000];
    let head = usize::from(PAYLOAD_FIRST_BANK) * 1024 + 0x1000;
    image[..0xE60].copy_from_slice(chr.get(head..head + 0xE60)?);
    let tail = usize::from(PAYLOAD_TAIL_BANK) * 1024 + usize::from(PAYLOAD_TAIL_OFFSET);
    image[0xE60..0xFC0].copy_from_slice(chr.get(tail..tail + 0x160)?);
    Some(image)
}

/// `$7000-$7FFF` at the first area load after boot: the intro and title's payload,
/// CHR banks `$F0-$F7` copied to WRAM `$6000-$7FBF` (bank 2 `$98FB`), `$7FC0-$7FFF`
/// zero. Equal to the oracle's at the load (`$89FE`) of a fresh start.
#[must_use]
pub fn boot_image(chr: &[u8]) -> Option<[u8; 0x1000]> {
    let head = usize::from(BOOT_FIRST_BANK) * 1024 + 0x1000;
    let mut image = [0u8; 0x1000];
    image[..0xFC0].copy_from_slice(chr.get(head..head + 0xFC0)?);
    Some(image)
}

/// [`map_from_rom`] over `image`, the `$7000-$7FFF` the loader finds: the stream
/// decompressed into it, then the whole of it transposed into the map.
#[must_use]
pub fn map_over(
    prg: &[u8],
    chr: &[u8],
    area: usize,
    image: &mut [u8; 0x1000],
) -> Option<[[u8; MAP_COLUMNS]; ROM_MAP_ROWS]> {
    let (stream, count) = map_stream(prg, chr, area)?;
    decompress(&stream[6..], count, image);
    Some(transpose(image))
}

/// How many bytes of `$7000-$7FFF` area `area`'s map stream writes: map page `p` row
/// `k` comes from `$7000 + p * 256 + k * 16`, so the cells past the reach hold what
/// that WRAM held at load time.
#[must_use]
pub fn map_reach(prg: &[u8], chr: &[u8], area: usize) -> Option<usize> {
    let (stream, count) = map_stream(prg, chr, area)?;
    Some(decompress(&stream[6..], count, &mut [0; 0x1000]).min(0x1000))
}

/// Area `area`'s compressed map in the CHR and its byte count.
fn map_stream<'a>(prg: &[u8], chr: &'a [u8], area: usize) -> Option<(&'a [u8], usize)> {
    let bank = area_bank(prg, area)?;
    let ppu = area_ppu(prg, area)?;
    let base = usize::from(bank) * 1024;
    let window = chr.get(base..base.checked_add(0x2000)?)?;
    let stream = window.get(usize::from(ppu) & 0x1FFF..)?;
    if stream.len() < 6 {
        return None;
    }
    let count = (usize::from(stream[0]) << 8 | usize::from(stream[1])).checked_sub(4)?;
    Some((stream, count))
}

/// Decode the background attribute tables (`$7600-$79FF`) for one area from the
/// ROM's CHR, exactly as the auto-tiler does.
///
/// The auto-tiler is bank 02 `$8B9E`; its attribute pass is `$8E57`. It reads a
/// per-area definition from CHR (bank `$8ECF`, PPU address `$8EE1`) and turns the
/// metatile map ([`map_from_rom`]) into 16
/// attribute tables of [`ATTRIBUTE_BYTES`] bytes, one per 256 px page: each output
/// byte packs the 2-bit palette class of a 2x2 metatile block, top-left in bits
/// 0-1, top-right 2-3, bottom-left 4-5, bottom-right 6-7 (`$C6F2[x]` is just
/// `(x & 15) << 4`, measured from the trace).
///
/// The definition (`docs/LEVELDATA.md` §13d) is:
///
/// 1. byte 2 = number of metatile ids `N`;
/// 2. bytes 3-17 + a forced `$12` = the 16-byte handler-shape table at `$04A0`;
/// 3. `ceil(N/2)` bytes of per-pair handler nibbles (even id = high nibble, odd
///    = low) feeding that table;
/// 4. one handler's worth of tile bytes per metatile, in id order (shape `s`
///    reads `HANDLER_READS[s]` bytes);
/// 5. `ceil(N/4)` bytes packing the classes four metatiles per byte, high pair
///    first.
///
/// Only metatile ids below `N` are level data; the rest of the map's columns
/// (160-255) are the level payload, and their attribute bytes are not generated.
///
/// Returns `None` when `prg`/`chr` are too short or `area` is out of range.
#[must_use]
pub fn attributes_from_rom(
    prg: &[u8],
    chr: &[u8],
    area: usize,
) -> Option<[[u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES]> {
    let mut image = payload_image(chr)?;
    let map = map_over(prg, chr, area, &mut image)?;
    attributes_of(prg, chr, area, &map, &image)
}

/// [`attributes_from_rom`] over a given map and the `$7000-$7FFF` the stream left
/// (the auto-tiler's pass `$8E57`): ids past the area's packed classes read their
/// class, unmasked, from whatever `$7B00` holds.
#[must_use]
pub fn attributes_of(
    prg: &[u8],
    chr: &[u8],
    area: usize,
    map: &[[u8; MAP_COLUMNS]; ROM_MAP_ROWS],
    image: &[u8; 0x1000],
) -> Option<[[u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES]> {
    let (classes, known) = classes_from_chr(prg, chr, area)?;
    let class = |id: u8| {
        let id = usize::from(id);
        if id < known {
            classes[id]
        } else {
            image[0xB00 + id]
        }
    };
    let mut pages = [[0u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES];
    for (page, table) in pages.iter_mut().enumerate() {
        for row_pair in 0..8 {
            for within in 0..8 {
                let row = row_pair * 2;
                let col = page * 16 + within * 2;
                table[row_pair * 8 + within] = class(map[row][col])
                    | class(map[row][col + 1]) << 2
                    | (class(map[row + 1][col]) & 0x0F) << 4
                    | (class(map[row + 1][col + 1]) & 0x03) << 6;
            }
        }
    }
    Some(pages)
}

/// The on-screen attribute table (`$23C0-$23FF`) at a scrolled camera.
///
/// The game keeps a 64-byte shadow of the table (`$7530`) and, each time the
/// camera crosses an 8 px tile step, replaces only the two quadrants of one
/// attribute column that belong to the metatile column entering at the right
/// edge (bank 02 `$8000-$8143`); the other two quadrants keep their old bytes.
/// Every quadrant therefore shows the page-table class of the metatile column
/// its ring slot holds, which depends on the camera alone.
#[must_use]
pub fn streamed_attributes(
    pages: &[[u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES],
    camera_x: u16,
) -> [u8; ATTRIBUTE_BYTES] {
    let left = (usize::from(camera_x) / 8) % (MAP_COLUMNS * 2);
    let mut table = [0u8; ATTRIBUTE_BYTES];
    for column in 0..8 {
        for half in 0..2 {
            let slot = column * 4 + half * 2;
            let tile =
                left + (slot + NAMETABLE_COLUMNS - left % NAMETABLE_COLUMNS) % NAMETABLE_COLUMNS;
            let metatile = (tile / 2) % MAP_COLUMNS;
            let mask = 0x33 << (metatile % 2 * 2);
            let source = &pages[metatile / 16];
            for row in 0..8 {
                table[row * 8 + column] |= source[row * 8 + metatile % 16 / 2] & mask;
            }
        }
    }
    table
}

/// The four 256-byte auto-tile tables (`$7000`, `$70D0`, `$71A0`, `$7270`) for
/// an area, decoded from the CHR area definition the auto-tiler's tile pass
/// reads (bank 02 `$8B9E`, jump table `$8C1D`; `docs/LEVELDATA.md` §13d).
///
/// Each metatile id's four definition bytes are turned into a 2x2 nametable
/// block by the handler the definition's nibble selects (the 19 handlers
/// `$8C43`-`$8E11`); the walk to each id's bytes is the same one
/// [`attributes_from_rom`] makes to skip them, so both agree on the layout. The
/// four tables overlap (stride `$D0`), and entries past the area's id count `N`
/// keep the map stream's bytes the loader left at `$7000` (`Level::from_rom` fills
/// them), so only ids `< N` are generated here.
///
/// Returns `None` when `prg`/`chr` are too short or `area` is out of range.
#[must_use]
pub fn auto_tile_tables(
    prg: &[u8],
    chr: &[u8],
    area: usize,
) -> Option<[[u8; TILE_TABLE_BYTES]; 4]> {
    let start = class_start(prg, area)?;
    let definition = chr.get(start..)?;
    let count = usize::from(*definition.get(2)?);

    let mut shape = [0u8; 16];
    shape[0] = 0x12;
    shape[1..].copy_from_slice(definition.get(3..18)?);

    let mut offset = 18usize.checked_add(count.div_ceil(2))?;
    let mut tables = [[0u8; TILE_TABLE_BYTES]; 4];
    for id in 0..count {
        let pair = *definition.get(18 + id / 2)?;
        let nibble = if id % 2 == 0 { pair >> 4 } else { pair & 0x0F };
        let handler = usize::from(*shape.get(usize::from(nibble))?);
        let reads = usize::from(*HANDLER_READS.get(handler)?);
        let corners = auto_tile_corners(handler, definition.get(offset..offset + reads)?)?;
        offset += reads;
        for (table, corner) in tables.iter_mut().zip(corners) {
            table[id] = corner;
        }
    }
    Some(tables)
}

/// How many metatile ids area `area`'s definition holds (its byte 2), the ids the
/// auto-tiler writes tile-table entries for.
#[must_use]
pub fn metatile_count(prg: &[u8], chr: &[u8], area: usize) -> Option<usize> {
    let start = class_start(prg, area)?;
    chr.get(start + 2).map(|&count| usize::from(count))
}

/// One metatile's 2x2 nametable block from its handler's definition bytes.
///
/// The handlers derive the four corners from the definition bytes by the small
/// offsets the disassembly shows (`+1`, `+$10`, `+$11`); corners are returned in
/// `[top-left, top-right, bottom-left, bottom-right]` order, the order of
/// [`TILE_TABLE_ADDRS`].
fn auto_tile_corners(handler: usize, bytes: &[u8]) -> Option<[u8; 4]> {
    let a = i32::from(*bytes.first()?);
    let b = i32::from(*bytes.get(1).unwrap_or(&0));
    let c = i32::from(*bytes.get(2).unwrap_or(&0));
    let d = i32::from(*bytes.get(3).unwrap_or(&0));
    let corners = match handler {
        0 => [a, a, a, a],
        1 => [a, a + 0x10, a + 1, a + 0x11],
        2 => [a, b, a, b],
        3 => [a, a, b, b],
        4 => [a, b, a + 1, b + 1],
        5 => [a, a + 0x10, b, b + 0x10],
        6 => [a, a, b, b + 0x10],
        7 => [a, a + 0x10, b, b],
        8 => [a, b, a + 1, b],
        9 => [a, b, a, b + 1],
        10 => [a, b, a, c],
        11 => [b, a, c, a],
        12 => [a, a, b, c],
        13 => [b, c, a, a],
        14 => [a, b, a + 1, c],
        15 => [b, a, c, a + 1],
        16 => [a, a + 0x10, b, c],
        17 => [b, c, a, a + 0x10],
        18 => [a, b, c, d],
        _ => return None,
    };
    Some(corners.map(|corner| u8::try_from(corner & 0xFF).expect("masked to a byte")))
}

/// The per-metatile 2-bit palette class table (`$7B00`) for one area, decoded
/// from the CHR area definition. See [`attributes_from_rom`] for the layout.
/// The packed 2-bit classes by id, and how many ids the packing covers (four a byte).
fn classes_from_chr(prg: &[u8], chr: &[u8], area: usize) -> Option<([u8; 256], usize)> {
    let start = class_start(prg, area)?;
    let definition = chr.get(start..)?;
    let count = usize::from(*definition.get(2)?);

    let mut shape = [0u8; 16];
    shape[0] = 0x12;
    shape[1..].copy_from_slice(definition.get(3..18)?);

    let mut offset = 18usize.checked_add(count.div_ceil(2))?;
    for id in 0..count {
        let pair = *definition.get(18 + id / 2)?;
        let nibble = if id % 2 == 0 { pair >> 4 } else { pair & 0x0F };
        let reads = usize::from(*HANDLER_READS.get(usize::from(shape[usize::from(nibble)]))?);
        offset = offset.checked_add(reads)?;
    }
    let packed = definition.get(offset..offset.checked_add(count.div_ceil(4))?)?;

    let known = count.div_ceil(4) * 4;
    let mut classes = [0u8; 256];
    for (id, class) in classes.iter_mut().enumerate().take(known) {
        *class = (packed[id / 4] >> (6 - 2 * (id % 4))) & 3;
    }
    Some((classes, known))
}

/// The area index (`$0400 / 2`) for a level index (`$20`) through
/// [`LEVEL_AREA_TABLE`], or `None` past its 13 entries. The mapping is verified
/// for every row (see [`LEVEL_AREA_TABLE`]).
#[must_use]
pub fn area_for_level(level: usize) -> Option<usize> {
    LEVEL_AREA_TABLE.get(level).map(|&v| usize::from(v) / 2)
}

/// Decode a level's map by its level index (`$20`), the value `sub_D3D8`
/// indexes [`LEVEL_AREA_TABLE`] with. This is the stage/sublevel -> CHR-set
/// entry point; see [`area_for_level`] for the table's status.
///
/// Returns `None` for an unknown level or when [`map_from_rom`] fails.
#[must_use]
pub fn map_for_level(
    prg: &[u8],
    chr: &[u8],
    level: usize,
) -> Option<[[u8; MAP_COLUMNS]; ROM_MAP_ROWS]> {
    map_from_rom(prg, chr, area_for_level(level)?)
}

/// The bank-02 `$89FE` bit-stream decompressor, emitting into `m7`
/// (`$7000-$7FFF`) with an LZ window `m6` (`$6000-$6FFF`).
fn decompress(stream: &[u8], count: usize, m7: &mut [u8; 0x1000]) -> usize {
    let mut m6 = [0u8; 0x1000];
    let mut dest = 0xFEEusize; // $6FEE, the LZ window write cursor
    let mut out = 0usize; // $7000 offset
    let mut pos = 0usize;
    let mut left = i64::try_from(count).unwrap_or(i64::MAX);
    let mut bits = 0u8;
    let mut nbits = 0u32;
    loop {
        if nbits == 0 {
            left -= 1;
            if left < 0 {
                break;
            }
            bits = stream.get(pos).copied().unwrap_or(0);
            pos += 1;
            nbits = 8;
        }
        let literal = bits & 1 == 1;
        bits >>= 1;
        nbits -= 1;
        if literal {
            let v = stream.get(pos).copied().unwrap_or(0);
            pos += 1;
            write(&mut m6, &mut dest, m7, &mut out, v);
        } else {
            let lo = stream.get(pos).copied().unwrap_or(0);
            pos += 1;
            left -= 1;
            if left < 0 {
                break;
            }
            let b2 = stream.get(pos).copied().unwrap_or(0);
            pos += 1;
            let mut src = (usize::from(b2 >> 4) << 8) | usize::from(lo);
            for _ in 0..(b2 & 0x0F) + 3 {
                let v = m6[src & 0xFFF];
                write(&mut m6, &mut dest, m7, &mut out, v);
                src = (src + 1) & 0xFFF;
            }
        }
        left -= 1;
        if left < 0 {
            break;
        }
    }
    out
}

/// One output byte: into the `$7000` result and the wrapping `$6000` LZ window.
fn write(m6: &mut [u8; 0x1000], dest: &mut usize, m7: &mut [u8; 0x1000], out: &mut usize, v: u8) {
    m6[*dest] = v;
    if let Some(slot) = m7.get_mut(*out) {
        *slot = v;
    }
    *out += 1;
    *dest = (*dest + 1) & 0xFFF;
}

/// Step 4 of the loader: `out[k*256+p*16+j] = in[p*256+k*16+j]`.
fn transpose(m7: &[u8; 0x1000]) -> [[u8; MAP_COLUMNS]; ROM_MAP_ROWS] {
    let mut out = [[0u8; MAP_COLUMNS]; ROM_MAP_ROWS];
    for (k, row) in out.iter_mut().enumerate() {
        for p in 0..16 {
            for j in 0..16 {
                row[p * 16 + j] = m7[p * 256 + k * 16 + j];
            }
        }
    }
    out
}

/// A WRAM image was too short to hold the map and the auto-tile tables.
#[derive(Debug, Error)]
pub enum LevelMapError {
    /// The dump did not reach the end of the last tile table.
    #[error("WRAM image is {actual} bytes, need {need} for the map and tile tables")]
    TooShort {
        /// Length supplied.
        actual: usize,
        /// Length required ([`TILE_TABLE_ADDRS`] last entry + its size).
        need: usize,
    },
}

/// The sublevel's metatile map and its auto-tile tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelMap {
    map: [[u8; MAP_COLUMNS]; MAP_ROWS],
    tables: [[u8; TILE_TABLE_BYTES]; 4],
}

impl LevelMap {
    /// Parse the map and tables out of a `$6000-$7FFF` WRAM image.
    ///
    /// # Errors
    /// [`LevelMapError::TooShort`] if `wram` does not reach the last table.
    pub fn from_wram(wram: &[u8]) -> Result<Self, LevelMapError> {
        let need = usize::from(TILE_TABLE_ADDRS[3] - MAP_ADDR) + TILE_TABLE_BYTES;
        if wram.len() < need {
            return Err(LevelMapError::TooShort {
                actual: wram.len(),
                need,
            });
        }
        let mut map = [[0u8; MAP_COLUMNS]; MAP_ROWS];
        for (row, dst) in map.iter_mut().enumerate() {
            let start = row * MAP_COLUMNS;
            dst.copy_from_slice(&wram[start..start + MAP_COLUMNS]);
        }
        let mut tables = [[0u8; TILE_TABLE_BYTES]; 4];
        for (table, dst) in tables.iter_mut().enumerate() {
            let start = usize::from(TILE_TABLE_ADDRS[table] - MAP_ADDR);
            dst.copy_from_slice(&wram[start..start + TILE_TABLE_BYTES]);
        }
        Ok(Self { map, tables })
    }

    /// The metatile id at `row`/`col`, or `None` if out of range.
    #[must_use]
    pub fn metatile(&self, row: usize, col: usize) -> Option<u8> {
        self.map.get(row)?.get(col).copied()
    }

    /// Render one physical nametable (`$2000-$23FF`) with the camera at
    /// `scroll_x`.
    ///
    /// Only the 30 tile rows the map covers are written; rows 30-31 stay `$00`,
    /// matching the captures.
    #[must_use]
    pub fn render_nametable(&self, scroll_x: u16) -> [u8; NAMETABLE_BYTES] {
        let tiles = MAP_COLUMNS * 2;
        let left = (usize::from(scroll_x) / 8) % tiles;
        let mut nametable = [0u8; NAMETABLE_BYTES];
        for p in 0..NAMETABLE_COLUMNS {
            let delta = (p + NAMETABLE_COLUMNS - left % NAMETABLE_COLUMNS) % NAMETABLE_COLUMNS;
            let column = (left + delta) % tiles;
            let half = column % 2;
            let metatile_col = column / 2;
            for row in 0..MAP_ROWS {
                let metatile = usize::from(self.map[row][metatile_col]);
                nametable[2 * row * NAMETABLE_COLUMNS + p] = self.tables[half][metatile];
                nametable[(2 * row + 1) * NAMETABLE_COLUMNS + p] = self.tables[2 + half][metatile];
            }
        }
        nametable
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rotj_oracle::{Machine, physics_input};

    use super::*;
    use crate::actor::oracle::Scene;
    use crate::digest::{fnv1a, table};

    /// A committed FNV-1a digest of an fceux capture's compared bytes
    /// (`tests/data/digests.txt`).
    fn digest(name: &str) -> u64 {
        table(include_str!("../tests/data/digests.txt"), name)["-"]
    }

    /// The oracle after the `physics` run's frame 699, i.e. the fceux frame-700 dump.
    fn physics_scene() -> Option<Scene> {
        let mut scene = Scene::power_on((0..=699).map(physics_input).collect())?;
        scene.run(699, &[]);
        Some(scene)
    }

    /// `tools/emu/dump.lua` `INPUT=drive`: Start 300-306; from 500 Right, A when
    /// `f % 90 < 3`, B when `f % 150 < 3`.
    fn drive_input(frame: usize) -> u8 {
        let mut pad = 0;
        if (300..=306).contains(&frame) {
            pad |= 0x08;
        }
        if frame >= 500 {
            pad |= 0x80;
            if frame % 90 < 3 {
                pad |= 0x01;
            }
            if frame % 150 < 3 {
                pad |= 0x02;
            }
        }
        pad
    }

    /// The oracle after `last` frames of the drive run (`last` = the dump frame - 1).
    fn drive_scene(last: u64) -> Option<Scene> {
        let mut scene = Scene::power_on(
            (0..=last)
                .map(|f| drive_input(usize::try_from(f).expect("frame fits")))
                .collect(),
        )?;
        scene.run(last, &[]);
        Some(scene)
    }

    /// The oracle's camera (`$CC:$CD >> 4`), floored to a tile step.
    fn camera(machine: &Machine) -> u16 {
        ((u16::from(machine.bus.ram[0xCC]) << 8) | u16::from(machine.bus.ram[0xCD])) >> 4 & !7
    }

    /// Count how many of the 30 visible tile rows of two nametables differ.
    fn visible_mismatches(a: &[u8], b: &[u8]) -> usize {
        (0..MAP_ROWS * 2)
            .flat_map(|row| (0..NAMETABLE_COLUMNS).map(move |col| row * 32 + col))
            .filter(|&i| a[i] != b[i])
            .count()
    }

    #[test]
    fn rejects_a_short_wram_image() {
        assert!(matches!(
            LevelMap::from_wram(&[0u8; 16]),
            Err(LevelMapError::TooShort { .. })
        ));
    }

    /// Acceptance: the decoded nametable equals fceux's over the 30 visible tile
    /// rows, at every camera position fceux captured (pinned by digest).
    ///
    /// The scroll-0 frame is the `physics` capture (`tools/emu/stage`, frame 700);
    /// the scrolled frames are `tools/emu/drive` 600/700/800, camera `$00AC` =
    /// 0/50/54 (`docs/LEVELDATA.md` §12b). Each oracle nametable is pinned to the
    /// capture by digest, then the port's render compared with it byte for byte.
    #[test]
    fn renders_every_captured_camera_position() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let (Some(drive600), Some(drive700), Some(drive800)) =
            (drive_scene(599), drive_scene(699), drive_scene(799))
        else {
            return;
        };
        let map = LevelMap::from_wram(&physics.machine.bus.wram[..]).unwrap();
        for (name, scroll, scene) in [
            ("stage-1-1-page", 0u16, &physics),
            ("stage-1-1-drive-600", 0, &drive600),
            ("stage-1-1-drive-700", 50, &drive700),
            ("stage-1-1-drive-800", 54, &drive800),
        ] {
            let nametable = &scene.machine.bus.ppu.vram;
            assert_eq!(
                fnv1a(&nametable[..960]),
                digest(name),
                "{name}: the oracle's nametable differs from fceux's"
            );
            let rendered = map.render_nametable(scroll);
            assert_eq!(
                visible_mismatches(&rendered, nametable),
                0,
                "{name} at scroll {scroll}"
            );
        }
    }

    #[test]
    fn scroll_advances_the_window_and_wraps_at_the_map_end() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let map = LevelMap::from_wram(&physics.machine.bus.wram[..]).unwrap();
        // Scrolling moves the visible window on, so the nametable changes...
        assert_ne!(map.render_nametable(0), map.render_nametable(8));
        assert_ne!(map.render_nametable(0), map.render_nametable(256));
        // ...and the 4096 px map repeats after its full width.
        assert_eq!(map.render_nametable(0), map.render_nametable(4096));
    }

    #[test]
    fn width_covers_the_whole_map() {
        assert_eq!(LEVEL_WIDTH_PIXELS, 4096);
    }

    /// Acceptance: the ROM decoder reproduces the whole `$6000-$6FFF` map the
    /// emulator holds at frame 700, byte-for-byte, including the payload-code
    /// columns 160-255.
    #[test]
    fn rom_decoder_reproduces_the_captured_map() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let chr = &physics.machine.bus.cart.chr;
        let prg = &physics.machine.bus.cart.prg;
        let captured = &physics.machine.bus.wram[..0x1000];
        assert_eq!(
            fnv1a(captured),
            digest("stage-1-1-map"),
            "the oracle's map differs from fceux's"
        );
        let map = map_from_rom(prg, chr, 0).expect("area 0 decodes");
        let diff = map
            .iter()
            .flatten()
            .zip(captured)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diff, 0, "ROM map vs the oracle's $6000-$6FFF");
    }

    /// The real level is 160 metatile columns wide: the stream stops after 10
    /// 16-column strips and the transpose copies the level payload into the
    /// remaining columns. This pins that fact so the garbage is not mistaken for
    /// level data.
    #[test]
    fn rom_map_columns_beyond_160_are_payload_code() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let chr = &physics.machine.bus.cart.chr;
        let prg = &physics.machine.bus.cart.prg;
        let captured = &physics.machine.bus.wram[..0x1000];
        assert_eq!(fnv1a(captured), digest("stage-1-1-map"));
        let map = map_from_rom(prg, chr, 0).unwrap();
        // Columns 0-159 are level data (and match); 160-255 match because both
        // are the payload, not because they are level.
        for (r, row) in map.iter().enumerate() {
            assert!(row[..160].iter().any(|&b| b != 0));
            assert_eq!(
                row.as_slice(),
                &captured[r * MAP_COLUMNS..(r + 1) * MAP_COLUMNS]
            );
        }
    }

    /// Every area in the fixed, 18-entry table decodes without error and yields
    /// a non-empty map, i.e. the decoder generalises past the acceptance screen.
    /// (Which area a given stage/sublevel uses is a lead: see
    /// `docs/LEVELDATA.md` §13e.)
    #[test]
    fn rom_decoder_decodes_every_area() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        for area in 0..AREA_COUNT {
            let map = map_from_rom(&rom.prg, chr, area).unwrap_or_else(|| panic!("area {area}"));
            assert!(
                map.iter().flatten().any(|&b| b != 0),
                "area {area} decoded empty"
            );
        }
        assert_eq!(AREA_COUNT, 18, "the bank-2 table is 18 bytes");
        assert!(map_from_rom(&rom.prg, chr, AREA_COUNT).is_none());
    }

    /// Acceptance: the generated attribute map equals the emulator's WRAM
    /// `$7600-$79FF` over the real level's 160 metatile columns (10 pages). The
    /// capture is the `$6000-$7FFF` dump at frame 700, so `$7600` is at offset
    /// `0x1600`.
    ///
    /// Columns 160-255 are the level payload, not level data; the game's own
    /// `$7B00` class bytes past the level's id count are stale, so those
    /// attribute bytes are run-dependent and are not required to match.
    #[test]
    fn rom_attributes_reproduce_the_captured_attribute_map() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let prg = &physics.machine.bus.cart.prg;
        let chr = &physics.machine.bus.cart.chr;
        let captured = &physics.machine.bus.wram[0x1600..0x1A00];
        assert_eq!(
            fnv1a(captured),
            digest("stage-1-1-attrs"),
            "the oracle's $7600 differs from fceux's"
        );
        let pages = attributes_from_rom(prg, chr, 0).expect("area 0 attributes");
        let real_pages = 10;
        let mut real_diff = 0;
        let mut total_diff = 0;
        for (page, attribute) in pages.iter().enumerate() {
            for (i, &value) in attribute.iter().enumerate() {
                if value != captured[page * ATTRIBUTE_BYTES + i] {
                    total_diff += 1;
                    if page < real_pages {
                        real_diff += 1;
                    }
                }
            }
        }
        println!(
            "-- attributes vs the oracle's $7600: {real_diff}/{real} real-level bytes differ; \
             {total_diff}/{total} total (pages 10-15 are payload columns)",
            real = real_pages * ATTRIBUTE_BYTES,
            total = ATTRIBUTE_PAGES * ATTRIBUTE_BYTES
        );
        assert_eq!(real_diff, 0, "attribute map over the real level width");
    }

    /// The generated attribute table for the page the captured camera sits on
    /// equals the emulator's PPU attribute table (`$23C0-$23FF`): at scroll 0 the
    /// nametable's attributes are the level page's own, byte for byte.
    #[test]
    fn rom_attributes_reproduce_the_captured_ppu_attribute_table() {
        let Some(physics) = physics_scene() else {
            return;
        };
        let prg = &physics.machine.bus.cart.prg;
        let chr = &physics.machine.bus.cart.chr;
        let captured = &physics.machine.bus.ppu.vram[960..1024];
        assert_eq!(
            fnv1a(captured),
            digest("stage-1-1-ppu-attrs"),
            "the oracle's $23C0 differs from fceux's"
        );
        let pages = attributes_from_rom(prg, chr, 0).unwrap();
        assert_eq!(&pages[0][..], captured);
    }

    /// Every area decodes its attribute map without error, i.e. the class-table
    /// walk generalises past the acceptance screen.
    #[test]
    fn rom_attributes_decode_every_area() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        for area in 0..AREA_COUNT {
            attributes_from_rom(&rom.prg, chr, area).unwrap_or_else(|| panic!("area {area}"));
        }
        assert!(attributes_from_rom(&rom.prg, chr, AREA_COUNT).is_none());
    }

    /// 65 distinct tile steps (camera 0-512 px, left and right moves) taken from
    /// three scripted gameplay runs, 1662 frames in all: every frame with the
    /// same tile step showed the same bytes, and the generated table equals the
    /// captured `$23C0-$23FF` for each.
    ///
    /// The oracle source is `stage1-inputs.csv` (the research's nearest run): for
    /// each camera the most frequent non-zero `$23C0` table after the level has
    /// started (`$0402 != $FF`) is taken, and the 65 `(camera, table)` records are
    /// pinned to the fixture by digest.
    #[test]
    fn streamed_attributes_reproduce_every_scrolled_frame() {
        let Some(mut machine) = rotj_oracle::power_on() else {
            return;
        };
        let pads = rotj_oracle::read_inputs(&rotj_oracle::data_path("stage1-inputs.csv"));
        let pages = attributes_from_rom(&machine.bus.cart.prg, &machine.bus.cart.chr, 0).unwrap();

        let mut counts: BTreeMap<u16, BTreeMap<[u8; ATTRIBUTE_BYTES], usize>> = BTreeMap::new();
        let mut previous = camera(&machine);
        for &pad in pads.iter().skip(1) {
            machine.run_frame(pad);
            let table: [u8; ATTRIBUTE_BYTES] =
                machine.bus.ppu.vram[0x3C0..0x400].try_into().unwrap();
            if machine.bus.ram[0x402] != 0xFF && table != [0u8; ATTRIBUTE_BYTES] {
                *counts
                    .entry(previous)
                    .or_default()
                    .entry(table)
                    .or_default() += 1;
            }
            previous = camera(&machine);
        }

        let mut captured = Vec::new();
        let mut records = Vec::new();
        for step in (0..=512u16).step_by(8) {
            let table = counts
                .get(&step)
                .and_then(|by_table| {
                    by_table
                        .iter()
                        .max_by_key(|&(_, &count)| count)
                        .map(|(table, _)| *table)
                })
                .unwrap_or_else(|| panic!("camera {step} has no $23C0 table in the route"));
            captured.extend_from_slice(&step.to_le_bytes());
            captured.extend_from_slice(&table);
            records.push((step, table));
        }
        assert_eq!(
            fnv1a(&captured),
            digest("attrstream-scroll"),
            "the 65 oracle (camera, $23C0) records differ from fceux's"
        );

        let mut differing = 0;
        for (step, oracle) in &records {
            let ours = streamed_attributes(&pages, *step);
            differing += ours.iter().zip(oracle).filter(|(a, b)| a != b).count();
        }
        println!(
            "-- streamed attributes: {differing} differing bytes over {} steps",
            records.len()
        );
        assert_eq!(differing, 0);
    }

    /// Single dumps from earlier captures, the 8 the oracle reproduces (drive
    /// 700/800, route-dense 760/820/852/900, route-oam 850, route-step 1000);
    /// `pages 720`, `route-chain 900/1200` and `seek2 700` have no recoverable
    /// input and are dropped (reported to the owner). The camera is `$00AC`
    /// completed with the high bits of `$CC:$CD >> 4`. Exact except route-dense
    /// frame 852 (2 bytes: `$00AC` is rounded, the game steps on the unrounded
    /// camera).
    #[test]
    fn streamed_attributes_against_earlier_captures() {
        let Some(chr_rom) = rotj_rom::for_test() else {
            return;
        };
        let pages = attributes_from_rom(&chr_rom.prg, &chr_rom.chr, 0).unwrap();
        let (Some(drive700), Some(drive800)) = (drive_scene(699), drive_scene(799)) else {
            return;
        };
        let Some(route760) = Scene::replay("route-inputs.csv", 759) else {
            return;
        };
        let Some(route820) = Scene::replay("route-inputs.csv", 819) else {
            return;
        };
        let Some(route852) = Scene::replay("route-inputs.csv", 851) else {
            return;
        };
        let Some(route900) = Scene::replay("route-inputs.csv", 899) else {
            return;
        };
        let Some(route1000) = Scene::replay("route-inputs.csv", 999) else {
            return;
        };
        let Some(route850) = Scene::replay("route-inputs.csv", 849) else {
            return;
        };
        let cases: [(u16, &Scene, usize); 8] = [
            (50, &drive700, 0),
            (54, &drive800, 0),
            (73, &route760, 0),
            (109, &route820, 0),
            (152, &route852, 2),
            (231, &route900, 0),
            (246, &route1000, 0),
            (148, &route850, 0),
        ];
        let table = |scene: &Scene| -> [u8; ATTRIBUTE_BYTES] {
            scene.machine.bus.ppu.vram[0x3C0..0x400].try_into().unwrap()
        };
        let mut captured = Vec::new();
        for (step, scene, _) in &cases {
            captured.extend_from_slice(&step.to_le_bytes());
            captured.extend_from_slice(&table(scene));
        }
        assert_eq!(
            fnv1a(&captured),
            digest("attrstream-dumps"),
            "the 8 oracle (camera, $23C0) records differ from fceux's"
        );
        for (step, scene, want) in &cases {
            let oracle = table(scene);
            let ours = streamed_attributes(&pages, *step);
            let differing = ours.iter().zip(&oracle).filter(|(a, b)| a != b).count();
            println!("-- camera {step}: {differing}/{ATTRIBUTE_BYTES} differ");
            assert_eq!(differing, *want, "camera {step}");
        }
    }

    /// The class table is read from the CHR area definition: area 0's decoded
    /// classes match the `$7B00` image the emulator's auto-tiler built, at the
    /// auto-tiler's `$8E57` (`tools/emu/attributes-trace`, frame 606).
    #[test]
    fn rom_classes_reproduce_the_emulator_class_table() {
        let Some(mut scene) = Scene::power_on((0..=607).map(physics_input).collect()) else {
            return;
        };
        let events = scene.run(607, &[0x8E57]);
        let captured = &events.first().expect("a $8E57 peek").wram[0x1B00..0x1B00 + 201];
        assert_eq!(
            fnv1a(captured),
            digest("classes-7b00"),
            "the oracle's $7B00 differs from fceux's"
        );
        let (classes, _) =
            classes_from_chr(&scene.machine.bus.cart.prg, &scene.machine.bus.cart.chr, 0)
                .expect("area 0 classes");
        assert_eq!(&classes[..201], captured);
    }

    /// The auto-tiler decoder (bank 02 `$8B9E`/`$8C1D`) reproduces the emulator's
    /// own `$7000`/`$70D0`/`$71A0`/`$7270` tile tables: area 0 against the frame-700
    /// WRAM, area 1 against the stage-1-2 frame-744 WRAM. Only the level's own ids
    /// (`< N`, the definition's count) are compared; the game's tables past `N` are
    /// stale from the previous level.
    #[test]
    fn auto_tiler_reproduces_the_captured_tile_tables() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        let prg = &rom.prg;
        // The four tables overlap (stride $D0), so each is read at its own
        // address in a captured slice.
        let ids = |area: usize| {
            let at = class_start(prg, area).expect("area definition") + 2;
            usize::from(chr[at])
        };

        let Some(physics) = physics_scene() else {
            return;
        };
        let wram = &physics.machine.bus.wram;
        let tables = auto_tile_tables(prg, chr, 0).expect("area 0 tables");
        let mut captured = Vec::new();
        for (table, addr) in TILE_TABLE_ADDRS.iter().enumerate() {
            let at = usize::from(*addr - MAP_ADDR);
            assert_eq!(
                &tables[table][..ids(0)],
                &wram[at..at + ids(0)],
                "area 0 table {table}"
            );
            captured.extend_from_slice(&wram[at..at + ids(0)]);
        }
        assert_eq!(
            fnv1a(&captured),
            digest("stage-1-1-tables"),
            "the oracle's area 0 tables differ from fceux's"
        );

        let Some(level1) = Scene::level(1, 743) else {
            return;
        };
        let wram = &level1.machine.bus.wram;
        let tables = auto_tile_tables(prg, chr, 1).expect("area 1 tables");
        let mut captured = Vec::new();
        for (table, addr) in TILE_TABLE_ADDRS.iter().enumerate() {
            let at = usize::from(*addr - MAP_ADDR);
            assert_eq!(
                &tables[table][..ids(1)],
                &wram[at..at + ids(1)],
                "area 1 table {table}"
            );
            captured.extend_from_slice(&wram[at..at + ids(1)]);
        }
        assert_eq!(
            fnv1a(&captured),
            digest("stage-1-2-tables"),
            "the oracle's area 1 tables differ from fceux's"
        );
    }

    /// A second area's attributes: area 16 (bank `$EA`) against the `$7600` of
    /// the forced level-12 load (fceux's `tools/emu/sublevels-force/wram-a16.bin`, by
    /// digest; the oracle's forced load gives the bytes), over
    /// every 2x2 block whose metatile ids are level data (`< N`).
    #[test]
    fn rom_attributes_reproduce_a_second_area() {
        let Some(level) = Scene::level(12, 623) else {
            return;
        };
        let prg = &level.machine.bus.cart.prg;
        let chr = &level.machine.bus.cart.chr;
        let area = 16;
        let captured = &level.machine.bus.wram[0x1600..0x1A00];
        assert_eq!(
            fnv1a(captured),
            digest("area16-attrs"),
            "the oracle's area 16 $7600 differs from fceux's"
        );
        let pages = attributes_from_rom(prg, chr, area).expect("area 16 attributes");
        let map = map_from_rom(prg, chr, area).expect("area 16 map");
        let start = class_start(prg, area).expect("area 16 definition");
        let count = chr[start + 2];
        let (mut checked, mut diff) = (0, 0);
        for (page, attribute) in pages.iter().enumerate() {
            for (i, &value) in attribute.iter().enumerate() {
                let (row, col) = (i / 8 * 2, page * 16 + i % 8 * 2);
                let ids = [
                    map[row][col],
                    map[row][col + 1],
                    map[row + 1][col],
                    map[row + 1][col + 1],
                ];
                if ids.iter().all(|&id| id < count) {
                    checked += 1;
                    diff += usize::from(value != captured[page * ATTRIBUTE_BYTES + i]);
                }
            }
        }
        assert_eq!((checked, diff), (1005, 0));
    }

    /// Stage 1-1's level index is 0, so its area is 0 (`$0400 = $D94E[0] = 0`);
    /// the table's last entry is `$20` -> area 16, and a 14th level is out of
    /// range.
    #[test]
    fn area_for_level_follows_the_bank14_table() {
        assert_eq!(area_for_level(0), Some(0));
        assert_eq!(area_for_level(12), Some(16));
        assert_eq!(area_for_level(LEVEL_AREA_TABLE.len()), None);
    }

    /// `map_for_level` is `map_from_rom` through the level -> area table.
    #[test]
    fn map_for_level_matches_map_from_rom() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        let prg = &rom.prg;
        assert_eq!(map_for_level(prg, chr, 0), map_from_rom(prg, chr, 0));
        assert_eq!(map_for_level(prg, chr, 12), map_from_rom(prg, chr, 16));
    }
}
