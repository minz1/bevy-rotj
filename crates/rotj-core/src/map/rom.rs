//! ROM decoders for level maps, auto-tiler tables, attribute maps, and collision classes.
//!
//! Everything here was verified against fceux's stage 1-1 captures and the emulator dumps,
//! which the tests reproduce with the oracle and pin by digest (`docs/re/LEVELDATA.md`).

use super::{
    ATTRIBUTE_BYTES, ATTRIBUTE_PAGES, CLASS_COUNT, MAP_COLUMNS, Map, NAMETABLE_COLUMNS,
    ROM_MAP_ROWS, TILE_TABLE_BYTES,
};
use crate::prg::{byte, word};
pub use crate::sim::spawn::{
    ActorError, CELL_PENDING, CELL_RECORD, SPAWN_BANK, SPAWN_SCHEDULE, SPAWN_TABLE, SpawnRecord,
    spawn_record, spawn_record_pointer, spawn_schedule,
};

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
/// by the map loader `$89FE` at `$8B63 + $0400/2`.
const AREA_BANK: usize = 0x8B63;

/// Bank 02 `$8B75`/`$8B76`: the PPU address the area's map stream is read from,
/// one 16-bit little-endian entry per area (`$8B75-$8B98`), matching [`AREA_BANK`].
const AREA_PPU: usize = 0x8B75;

/// Bank 14 offset `$194E` = CPU `$D94E`: the level index (`$20`) -> `$0400`
/// value, read at level start by `bank0E:sub_D3D8`.
pub const LEVEL_AREA_TABLE: [u8; 13] = [
    0x00, 0x02, 0x06, 0x08, 0x0A, 0x0C, 0x10, 0x12, 0x16, 0x18, 0x1A, 0x1C, 0x20,
];

/// End of the main payload copy in the `$7000-$7FFF` image (`$7E60`).
const WRAM_HEAD_END: usize = 0xE60;
/// Length of the payload tail copied into `$7E60-$7FBF`.
const WRAM_TAIL_LEN: usize = 0x160;
/// End of the payload / boot copy in `$7000-$7FFF` (`$7FC0`).
const WRAM_TAIL_END: usize = 0xFC0;
/// Offset in `$7000-$7FFF` of the palette-class buffer (`$7B00`).
const WRAM_CLASSES_OFFSET: usize = 0xB00;

/// Bank 02 `$8ECF`: the CHR 1 KiB base bank the auto-tiler (`$8B9E`) reads its area
/// definition from, one entry per area (`$0400 / 2`).
const CLASS_BANK: usize = 0x8ECF;

/// Bank 02 `$8EE1`/`$8EE2`: the PPU address of the area definition the auto-tiler reads
/// through `$2007`.
const CLASS_PPU: usize = 0x8EE1;

/// Bank 14 `$C000`: each area's class-table pointer, read as 16-bit little-endian
/// values indexed by the raw `$0400` value / 2.
const CLASS_PTR_TABLE: usize = 0xC000;

/// Bank 0 offset `$02A5`: where the run-length class blobs begin.
const CLASS_RLE_BASE: usize = 0x02A5;

/// Bytes of the class blobs (`$02A5-$0322`).
const CLASS_RLE_BYTES: usize = 126;

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

/// Auto-tiler corner-expansion handler (`$8C1D`, 19 entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handler {
    RepeatAll,
    Block2x2,
    Columns,
    Rows,
    VerticalPairs,
    HorizontalPairs,
    TopSameBottomPair,
    TopPairBottomSame,
    LeftPairRightSame,
    LeftSameRightPair,
    RepeatLeft,
    RepeatRight,
    RepeatTop,
    RepeatBottom,
    LeftPairFreeRight,
    RightPairFreeLeft,
    TopPairFreeBottom,
    BottomPairFreeTop,
    FourTiles,
}

impl Handler {
    const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::RepeatAll),
            1 => Some(Self::Block2x2),
            2 => Some(Self::Columns),
            3 => Some(Self::Rows),
            4 => Some(Self::VerticalPairs),
            5 => Some(Self::HorizontalPairs),
            6 => Some(Self::TopSameBottomPair),
            7 => Some(Self::TopPairBottomSame),
            8 => Some(Self::LeftPairRightSame),
            9 => Some(Self::LeftSameRightPair),
            10 => Some(Self::RepeatLeft),
            11 => Some(Self::RepeatRight),
            12 => Some(Self::RepeatTop),
            13 => Some(Self::RepeatBottom),
            14 => Some(Self::LeftPairFreeRight),
            15 => Some(Self::RightPairFreeLeft),
            16 => Some(Self::TopPairFreeBottom),
            17 => Some(Self::BottomPairFreeTop),
            18 => Some(Self::FourTiles),
            _ => None,
        }
    }

    const fn reads(self) -> usize {
        match self {
            Self::RepeatAll | Self::Block2x2 => 1,
            Self::Columns
            | Self::Rows
            | Self::VerticalPairs
            | Self::HorizontalPairs
            | Self::TopSameBottomPair
            | Self::TopPairBottomSame
            | Self::LeftPairRightSame
            | Self::LeftSameRightPair => 2,
            Self::RepeatLeft
            | Self::RepeatRight
            | Self::RepeatTop
            | Self::RepeatBottom
            | Self::LeftPairFreeRight
            | Self::RightPairFreeLeft
            | Self::TopPairFreeBottom
            | Self::BottomPairFreeTop => 3,
            Self::FourTiles => 4,
        }
    }

    fn corners(self, bytes: &[u8]) -> Option<[u8; 4]> {
        let a = *bytes.first()?;
        let b = bytes.get(1).copied().unwrap_or(0);
        let c = bytes.get(2).copied().unwrap_or(0);
        let d = bytes.get(3).copied().unwrap_or(0);
        Some(match self {
            Self::RepeatAll => [a, a, a, a],
            Self::Block2x2 => [
                a,
                a.wrapping_add(0x10),
                a.wrapping_add(1),
                a.wrapping_add(0x11),
            ],
            Self::Columns => [a, b, a, b],
            Self::Rows => [a, a, b, b],
            Self::VerticalPairs => [a, b, a.wrapping_add(1), b.wrapping_add(1)],
            Self::HorizontalPairs => [a, a.wrapping_add(0x10), b, b.wrapping_add(0x10)],
            Self::TopSameBottomPair => [a, a, b, b.wrapping_add(0x10)],
            Self::TopPairBottomSame => [a, a.wrapping_add(0x10), b, b],
            Self::LeftPairRightSame => [a, b, a.wrapping_add(1), b],
            Self::LeftSameRightPair => [a, b, a, b.wrapping_add(1)],
            Self::RepeatLeft => [a, b, a, c],
            Self::RepeatRight => [b, a, c, a],
            Self::RepeatTop => [a, a, b, c],
            Self::RepeatBottom => [b, c, a, a],
            Self::LeftPairFreeRight => [a, b, a.wrapping_add(1), c],
            Self::RightPairFreeLeft => [b, a, c, a.wrapping_add(1)],
            Self::TopPairFreeBottom => [a, a.wrapping_add(0x10), b, c],
            Self::BottomPairFreeTop => [b, c, a, a.wrapping_add(0x10)],
            Self::FourTiles => [a, b, c, d],
        })
    }
}

struct AreaDefinitions<'a> {
    count: usize,
    entries: Vec<(Handler, &'a [u8])>,
    packed_classes: &'a [u8],
}

fn area_definitions<'a>(prg: &[u8], chr: &'a [u8], area: usize) -> Option<AreaDefinitions<'a>> {
    let start = class_start(prg, area)?;
    let definition = chr.get(start..)?;
    let count = usize::from(*definition.get(2)?);

    let mut shape = [0u8; 16];
    shape[0] = 0x12;
    shape[1..].copy_from_slice(definition.get(3..18)?);

    let mut offset = 18usize.checked_add(count.div_ceil(2))?;
    let mut entries = Vec::with_capacity(count);
    for id in 0..count {
        let pair = *definition.get(18 + id / 2)?;
        let nibble = if id % 2 == 0 { pair >> 4 } else { pair & 0x0F };
        let handler = Handler::from_byte(*shape.get(usize::from(nibble))?)?;
        let reads = handler.reads();
        let bytes = definition.get(offset..offset.checked_add(reads)?)?;
        offset += reads;
        entries.push((handler, bytes));
    }
    let packed_classes = definition.get(offset..offset.checked_add(count.div_ceil(4))?)?;
    Some(AreaDefinitions {
        count,
        entries,
        packed_classes,
    })
}

/// Decode one area's 208-byte collision class table (`$7340`) from the ROM bytes.
#[must_use]
pub fn class_table(prg: &[u8], area: usize) -> Option<[u8; CLASS_COUNT]> {
    if area >= AREA_COUNT {
        return None;
    }
    let ptr = word(prg, 14, CLASS_PTR_TABLE + area * 2)? & 0x1FFF;
    let blobs = prg.get(CLASS_RLE_BASE..CLASS_RLE_BASE + CLASS_RLE_BYTES)?;
    let mut at = usize::from(ptr).checked_sub(CLASS_RLE_BASE)?;
    let mut table = [0u8; CLASS_COUNT];
    let mut id = 0;
    while id < CLASS_COUNT {
        let value = *blobs.get(at)?;
        let run = *blobs.get(at + 1)?;
        at += 2;
        if run == 0 {
            return None;
        }
        for _ in 0..run {
            if id >= CLASS_COUNT {
                break;
            }
            table[id] = value;
            id += 1;
        }
    }
    Some(table)
}

/// Decode the metatile map (`$6000-$6FFF`, [`ROM_MAP_ROWS`] x [`MAP_COLUMNS`])
/// for one area straight from the ROM's CHR, exactly as the game's loader does.
#[must_use]
pub fn map_from_rom(prg: &[u8], chr: &[u8], area: usize) -> Option<Map> {
    let mut image = payload_image(chr)?;
    map_over(prg, chr, area, &mut image)
}

/// `$7000-$7FFF` as the payload leaves them: CHR bank `$F8` offset `$1000` (bank
/// `$FC`), then the tail from bank `$F7` offset `$260`; `$7FC0-$7FFF` zero.
#[must_use]
pub fn payload_image(chr: &[u8]) -> Option<[u8; 0x1000]> {
    let mut image = [0u8; 0x1000];
    let head = usize::from(PAYLOAD_FIRST_BANK) * 1024 + 0x1000;
    image[..WRAM_HEAD_END].copy_from_slice(chr.get(head..head + WRAM_HEAD_END)?);
    let tail = usize::from(PAYLOAD_TAIL_BANK) * 1024 + usize::from(PAYLOAD_TAIL_OFFSET);
    image[WRAM_HEAD_END..WRAM_TAIL_END].copy_from_slice(chr.get(tail..tail + WRAM_TAIL_LEN)?);
    Some(image)
}

/// `$7000-$7FFF` at the first area load after boot: the intro and title's payload,
/// CHR banks `$F0-$F7` copied to WRAM `$6000-$7FBF` (bank 2 `$98FB`), `$7FC0-$7FFF`
/// zero. Equal to the oracle's at the load (`$89FE`) of a fresh start.
#[must_use]
pub fn boot_image(chr: &[u8]) -> Option<[u8; 0x1000]> {
    let head = usize::from(BOOT_FIRST_BANK) * 1024 + 0x1000;
    let mut image = [0u8; 0x1000];
    image[..WRAM_TAIL_END].copy_from_slice(chr.get(head..head + WRAM_TAIL_END)?);
    Some(image)
}

/// [`map_from_rom`] over `image`, the `$7000-$7FFF` the loader finds: the stream
/// decompressed into it, then the whole of it transposed into the map.
#[must_use]
pub fn map_over(prg: &[u8], chr: &[u8], area: usize, image: &mut [u8; 0x1000]) -> Option<Map> {
    let (stream, count) = map_stream(prg, chr, area)?;
    decompress(&stream[6..], count, image);
    Some(transpose(image))
}

/// The bytes area `area`'s map stream writes over `$7000`, in that order: the cells past
/// them keep what that WRAM held at load time.
#[must_use]
pub fn streamed_map(prg: &[u8], chr: &[u8], area: usize) -> Option<Vec<u8>> {
    let (stream, count) = map_stream(prg, chr, area)?;
    let mut image = [0; 0x1000];
    let reach = decompress(&stream[6..], count, &mut image).min(0x1000);
    Some(image[..reach].to_vec())
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
    let count = usize::from(u16::from_be_bytes([stream[0], stream[1]])).checked_sub(4)?;
    Some((stream, count))
}

/// Decode the background attribute tables (`$7600-$79FF`) for one area from the
/// ROM's CHR, exactly as the auto-tiler does.
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
/// (the auto-tiler's pass `$8E57`).
#[must_use]
pub fn attributes_of(
    prg: &[u8],
    chr: &[u8],
    area: usize,
    map: &Map,
    image: &[u8; 0x1000],
) -> Option<[[u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES]> {
    let (classes, known) = classes_from_chr(prg, chr, area)?;
    Some(attributes_from_classes(&classes[..known], map, image))
}

/// The attribute pages for a map from its metatiles' palette classes (`palette_classes`,
/// by id) and the `$7000-$7FFF` image.
#[must_use]
pub fn attributes_from_classes(
    palette_classes: &[u8],
    map: &Map,
    image: &[u8; 0x1000],
) -> [[u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES] {
    let rows = map.rows();
    let class = |id: u8| {
        let id = usize::from(id);
        palette_classes
            .get(id)
            .copied()
            .unwrap_or(image[WRAM_CLASSES_OFFSET + id])
    };
    let mut pages = [[0u8; ATTRIBUTE_BYTES]; ATTRIBUTE_PAGES];
    for (page, table) in pages.iter_mut().enumerate() {
        for row_pair in 0..8 {
            for within in 0..8 {
                let row = row_pair * 2;
                let col = page * 16 + within * 2;
                table[row_pair * 8 + within] = class(rows[row][col])
                    | class(rows[row][col + 1]) << 2
                    | (class(rows[row + 1][col]) & 0x0F) << 4
                    | (class(rows[row + 1][col + 1]) & 0x03) << 6;
            }
        }
    }
    pages
}

/// The on-screen attribute table (`$23C0-$23FF`) at a scrolled camera.
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
/// an area, decoded from the CHR area definition the auto-tiler's tile pass reads.
#[must_use]
pub fn auto_tile_tables(
    prg: &[u8],
    chr: &[u8],
    area: usize,
) -> Option<[[u8; TILE_TABLE_BYTES]; 4]> {
    let definitions = area_definitions(prg, chr, area)?;
    let mut tables = [[0u8; TILE_TABLE_BYTES]; 4];
    for (id, (handler, bytes)) in definitions.entries.into_iter().enumerate() {
        let corners = handler.corners(bytes)?;
        for (table, corner) in tables.iter_mut().zip(corners) {
            table[id] = corner;
        }
    }
    Some(tables)
}

/// How many metatile ids area `area`'s definition holds (its byte 2).
#[must_use]
pub fn metatile_count(prg: &[u8], chr: &[u8], area: usize) -> Option<usize> {
    let start = class_start(prg, area)?;
    chr.get(start + 2).map(|&count| usize::from(count))
}

/// One area's 2-bit palette classes (`$7B00`) by id, decoded from the CHR area definition,
/// and how many ids the packing covers (four a byte).
#[must_use]
pub fn classes_from_chr(prg: &[u8], chr: &[u8], area: usize) -> Option<([u8; 256], usize)> {
    let definitions = area_definitions(prg, chr, area)?;
    let known = definitions.count.div_ceil(4) * 4;
    let mut classes = [0u8; 256];
    for (id, class) in classes.iter_mut().enumerate().take(known) {
        *class = (definitions.packed_classes[id / 4] >> (6 - 2 * (id % 4))) & 3;
    }
    Some((classes, known))
}

/// The area index (`$0400 / 2`) for a level index (`$20`) through
/// [`LEVEL_AREA_TABLE`], or `None` past its 13 entries.
#[must_use]
pub fn area_for_level(level: usize) -> Option<usize> {
    LEVEL_AREA_TABLE.get(level).map(|&v| usize::from(v) / 2)
}

/// The bank-02 `$89FE` bit-stream decompressor, emitting into `m7`
/// (`$7000-$7FFF`) with an LZ window `m6` (`$6000-$6FFF`).
fn decompress(stream: &[u8], count: usize, m7: &mut [u8; 0x1000]) -> usize {
    let mut m6 = [0u8; 0x1000];
    let mut dest = 0xFEEusize;
    let mut out = 0usize;
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
#[must_use]
pub fn transpose(m7: &[u8; 0x1000]) -> Map {
    let mut out = [[0u8; MAP_COLUMNS]; ROM_MAP_ROWS];
    for (k, row) in out.iter_mut().enumerate() {
        for p in 0..16 {
            for j in 0..16 {
                row[p * 16 + j] = m7[p * 256 + k * 16 + j];
            }
        }
    }
    Map::from_rows(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rotj_oracle::{Machine, physics_input};

    use super::*;
    use crate::digest::{fnv1a, table};
    use crate::map::{
        Level, MAP_ADDR, MAP_ROWS, Surface, TILE_TABLE_ADDRS, Terrain, streamed_cell,
    };
    use crate::sim::oracle::Scene;

    fn digest(name: &str) -> u64 {
        table(include_str!("../../tests/data/digests.txt"), name)["-"]
    }

    fn physics_scene() -> Option<Scene> {
        let mut scene = Scene::power_on((0..=699).map(physics_input).collect())?;
        scene.run(699, &[]);
        Some(scene)
    }

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

    fn drive_scene(last: u64) -> Option<Scene> {
        let mut scene = Scene::power_on(
            (0..=last)
                .map(|f| drive_input(usize::try_from(f).expect("frame fits")))
                .collect(),
        )?;
        scene.run(last, &[]);
        Some(scene)
    }

    fn camera(machine: &Machine) -> u16 {
        ((u16::from(machine.bus.ram[0xCC]) << 8) | u16::from(machine.bus.ram[0xCD])) >> 4 & !7
    }

    fn visible_mismatches(a: &[u8], b: &[u8]) -> usize {
        (0..MAP_ROWS * 2)
            .flat_map(|row| (0..NAMETABLE_COLUMNS).map(move |col| row * 32 + col))
            .filter(|&i| a[i] != b[i])
            .count()
    }

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
        let (images, _) = physics.machine.bus.wram.as_chunks::<0x1000>();
        let (map, tables) = (Map::from_bytes(&images[0]), &images[1]);
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
            let rendered = map.render_nametable(tables, scroll);
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
        let (images, _) = physics.machine.bus.wram.as_chunks::<0x1000>();
        let (map, tables) = (Map::from_bytes(&images[0]), &images[1]);
        assert_ne!(
            map.render_nametable(tables, 0),
            map.render_nametable(tables, 8)
        );
        assert_ne!(
            map.render_nametable(tables, 0),
            map.render_nametable(tables, 256)
        );
        assert_eq!(
            map.render_nametable(tables, 0),
            map.render_nametable(tables, 4096)
        );
    }

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
            .as_bytes()
            .iter()
            .zip(captured)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diff, 0, "ROM map vs the oracle's $6000-$6FFF");
    }

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
        for (r, row) in map.rows().iter().enumerate() {
            assert!(row[..160].iter().any(|&b| b != 0));
            assert_eq!(
                row.as_slice(),
                &captured[r * MAP_COLUMNS..(r + 1) * MAP_COLUMNS]
            );
        }
    }

    #[test]
    fn rom_decoder_decodes_every_area() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        for area in 0..AREA_COUNT {
            let map = map_from_rom(&rom.prg, chr, area).unwrap_or_else(|| panic!("area {area}"));
            assert!(
                map.as_bytes().iter().any(|&b| b != 0),
                "area {area} decoded empty"
            );
        }
        assert_eq!(AREA_COUNT, 18, "the bank-2 table is 18 bytes");
        assert!(map_from_rom(&rom.prg, chr, AREA_COUNT).is_none());
    }

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

    #[test]
    fn auto_tiler_reproduces_the_captured_tile_tables() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let chr = &rom.chr;
        let prg = &rom.prg;
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
                    map.rows()[row][col],
                    map.rows()[row][col + 1],
                    map.rows()[row + 1][col],
                    map.rows()[row + 1][col + 1],
                ];
                if ids.iter().all(|&id| id < count) {
                    checked += 1;
                    diff += usize::from(value != captured[page * ATTRIBUTE_BYTES + i]);
                }
            }
        }
        assert_eq!((checked, diff), (1005, 0));
    }

    #[test]
    fn area_for_level_follows_the_bank14_table() {
        assert_eq!(area_for_level(0), Some(0));
        assert_eq!(area_for_level(12), Some(16));
        assert_eq!(area_for_level(LEVEL_AREA_TABLE.len()), None);
    }

    fn loaded_from_rom(
        prg: &[u8],
        chr: &[u8],
        area: usize,
        mut image: [u8; 0x1000],
    ) -> Option<Level> {
        let map = map_over(prg, chr, area, &mut image)?;
        let mut classes = class_table(prg, area)?;
        for class in &mut classes {
            *class <<= 1;
        }
        let tables = auto_tile_tables(prg, chr, area)?;
        let ids = metatile_count(prg, chr, area)?;
        let corners = std::array::from_fn(|id| {
            std::array::from_fn(|table| {
                if id < ids {
                    tables[table][id]
                } else {
                    map[streamed_cell(table * CLASS_COUNT + id)]
                }
            })
        });
        let pages = attributes_of(prg, chr, area, &map, &image)?;
        let mut attributes = [0; 1024];
        for (bytes, page) in attributes.as_chunks_mut::<64>().0.iter_mut().zip(&pages) {
            *bytes = *page;
        }
        let unread = Level::pack_unread(&image);
        Some(Level {
            map,
            corners,
            classes,
            attributes,
            unread,
        })
    }

    #[test]
    fn terrain_builds_what_the_rom_decoder_left() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let synthetic: [u8; 0x1000] =
            std::array::from_fn(|i| u8::try_from((i * 37 + i / 256) % 251).expect("byte"));
        let images = [
            boot_image(&rom.chr).expect("the boot payload"),
            payload_image(&rom.chr).expect("the cutscene payload"),
            synthetic,
        ];
        let mut areas = 0;
        for area in 0..32 {
            let Some(terrain) = Terrain::import(&rom.prg, &rom.chr, area) else {
                continue;
            };
            areas += 1;
            for (which, &image) in images.iter().enumerate() {
                assert_eq!(
                    Some(Level::from_terrain(&terrain, image)),
                    loaded_from_rom(&rom.prg, &rom.chr, area, image),
                    "area {area}, image {which}"
                );
            }
        }
        assert_eq!(areas, 18, "every area imports");
    }

    const AREA_17_IDS: usize = 102;

    #[test]
    fn area_zero_table_matches_the_capture() {
        let Some(mut scene) = Scene::power_on((0..=699).map(rotj_oracle::physics_input).collect())
        else {
            return;
        };
        scene.run(699, &[]);
        let captured = &scene.machine.bus.wram[0x1340..0x1340 + CLASS_COUNT];
        assert_eq!(
            fnv1a(captured),
            digest("stage-1-1-class"),
            "the oracle's $7340 differs from fceux's"
        );
        let ours = class_table(&scene.machine.bus.cart.prg, 0).expect("area 0 has a table");
        for (id, (&stored, &class)) in captured.iter().zip(&ours).enumerate() {
            assert_eq!(stored, class * 2, "metatile {id:#04X} class");
        }
    }

    #[test]
    fn every_area_decodes() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        for area in 0..AREA_COUNT {
            let table = class_table(&rom.prg, area).unwrap_or_else(|| panic!("area {area}"));
            let used = if area == 17 { AREA_17_IDS } else { CLASS_COUNT };
            assert!(
                table[..used].iter().all(|&c| c <= 16),
                "area {area} has a class outside 0..=16"
            );
        }
    }

    #[test]
    fn surfaces_match_the_handler_tables() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        let handlers = |surface| match surface {
            Surface::Solid => (0xC9DA, 0xCB39, Some(0x80)),
            Surface::BeltRight => (0xCA2C, 0xCB80, Some(0x80)),
            Surface::BeltLeft => (0xCA3D, 0xCBB8, Some(0x80)),
            Surface::HazardBehind => (0xCA4E, 0xCBF0, Some(0)),
            Surface::SolidHazard => (0xCA66, 0xCC08, Some(0x80)),
            Surface::Slippery => (0xCA86, 0xCC1A, Some(0x80)),
            Surface::Behind => (0xCA97, 0xCC61, Some(0)),
            Surface::Open => (0xCAA0, 0xCC6A, Some(0)),
            Surface::SolidToBodies => (0xC9DA, 0xCB39, Some(0)),
            Surface::Crumbling => (0xCAA3, 0xCC6D, None),
            Surface::OneWay => (0xCAA6, 0xCCFC, None),
            Surface::OneWayBehind => (0xCA97, 0xCCFC, None),
            Surface::Spikes => (0xCAA9, 0xCD09, None),
        };
        for class in 0..=16u8 {
            let at = usize::from(class * 2);
            let surface = Surface::from_stored(class * 2).expect("a class");
            let point = if byte(prg, 14, 0xC97D + at).expect("the sign") & 0x80 == 0 {
                byte(prg, 14, 0xC97E + at)
            } else {
                None
            };
            let rom = (
                word(prg, 14, 0xCADC + at).expect("the wall handler"),
                word(prg, 14, 0xCD2E + at).expect("the floor handler"),
                point,
            );
            assert_eq!(rom, handlers(surface), "class {class}");
        }
        assert!(
            (0..=u8::MAX)
                .filter(|&stored| stored & 1 != 0 || stored > 32)
                .all(|stored| Surface::from_stored(stored).is_none())
        );
    }
}
