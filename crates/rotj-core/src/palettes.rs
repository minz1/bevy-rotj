//! A level's palette and gameplay CHR-bank set, decoded from the ROM.
//!
//! At level start the game turns a per-**(area, stage)** record into palette RAM
//! and the FME-7 CHR-bank shadow. This module reproduces that chain from the PRG
//! ROM bytes; `docs/LEVELDATA.md` §14 records the addresses and evidence.
//!
//! # The record (bank 0)
//!
//! The selector `$0400` (the area, `2 * area`) indexes a table in bank 14 at
//! `$C048` that points at a per-area **stage-pointer table** in bank 0
//! (`$8020`...). The value at `($C048[$0400])[$0402]` is the record for the
//! current stage; `sub_B40A` (bank 13 `$B40A`) reads a 7-byte record there:
//!
//! | byte | meaning |
//! | --- | --- |
//! | 0 | palette selector `Y` for `far_C367` |
//! | 1-4 | CHR 1 KiB banks `$E0-$E3`; a value `>= $80` is a **handle** resolved through `$C06C` |
//! | 5 | `$DD` (a level flag) |
//! | 6 | extra |
//!
//! # The palette (`far_C367`, bank 14)
//!
//! `far_C367` (`$C367`) takes two selectors, `X` and `Y`, reads one byte per
//! palette out of the per-palette selector tables at `$C3B6`... (pointed to by
//! `$C3A6`), and that byte indexes three colour tables `$C4BA`/`$C522`/`$C58A`
//! for the palette's colours 1-3. Colour 0 of each of the 8 palettes is `$0F`
//! (`sub_D481` in bank 14). `X` comes from a table at bank 13 `$B5F5` indexed by
//! the area and the stage; `Y` is record byte 0.
//!
//! # The CHR banks
//!
//! `sub_B40A` writes record bytes 1-4 to the shadow `$E0-$E3`; any byte with bit
//! 7 set is a handle: its group record lives at `$C06C[(handle<<1) & $7F]` and
//! the resolved bank is the last byte of that group. Banks `$E4-$E7` do not vary
//! by level: `$E4 = $A0` (`bank 13 $B22A`), `$E5-$E7 = $F5-$F7` (the boot
//! default set by bank 2 `$98CB`).
//!
//! # Scope
//!
//! The probes that verified this (`tools/emu/stagearea-probe/`, `docs/LEVELDATA.md`
//! §14) all ran with `$0402 = 0`, so only stage 0 is decoded. `$0402`'s exact
//! meaning (a stage counter, an already-doubled index) is a lead.

/// Palette-RAM bytes: 8 palettes of 4 colours.
pub const PALETTE_BYTES: usize = 32;
/// The FME-7's eight 1 KiB CHR banks (commands 0-7, shadow `$88-$8F`).
pub const CHR_BANKS: usize = 8;

/// FME-7 PRG window size: the unit `disasm/` and `docs/` call "bank".
const BANK_BYTES: usize = 8192;
/// While `sub_B40A` runs, `$8000` holds bank 0 (verified: the record pointers
/// land on bank 0's tables; `tools/emu/palettes-trace/`).
const RECORD_BANK: usize = 0;
/// `$A000` holds bank 13, where the palette `X`-selector table lives.
const XSELECT_BANK: usize = 13;
/// `$C000` holds bank 14, where the per-area and palette tables live.
const TABLE_BANK: usize = 14;

/// Bank 14 `$C048`: per-area pointer to the area's stage-pointer table.
const AREA_RECORD_TABLE: usize = 0xC048;
/// Bank 14 `$C06C`: a handle (`>= $80`) -> its CHR group record.
const GROUP_TABLE: usize = 0xC06C;
/// Bank 14 `$C3A6`: one pointer per palette into its selector table.
const PALETTE_POINTER_TABLE: usize = 0xC3A6;
/// Bank 14 `$C4BA`/`$C522`/`$C58A`: the colours 1, 2, 3 indexed by a selector.
const PALETTE_COLOR_TABLES: [usize; 3] = [0xC4BA, 0xC522, 0xC58A];
/// Bank 13 `$B5F5`: per-area pointer to the area's `X`-selector table.
const XSELECT_TABLE: usize = 0xB5F5;
/// Entries in the per-area `$C048` pointer table (same 18 areas as
/// [`crate::levelmap::area_bank`]).
const AREAS: usize = 18;

/// CHR banks `$E4-$E7`, constant for every level: `$E4 = $A0` (bank 13 `$B22A`),
/// `$E5-$E7 = $F5-$F7` (bank 2 `$98CB`).
const CHR_FIXED: [u8; 4] = [0xA0, 0xF5, 0xF6, 0xF7];

/// One PRG ROM byte, addressed inside an 8 KiB window mapped from `bank`.
fn read(prg: &[u8], bank: usize, addr: usize) -> Option<u8> {
    prg.get(bank.checked_mul(BANK_BYTES)? + (addr & 0x1FFF))
        .copied()
}

/// A little-endian 16-bit word, addressed as in [`read`].
fn read_word(prg: &[u8], bank: usize, addr: usize) -> Option<u16> {
    let lo = read(prg, bank, addr)?;
    let hi = read(prg, bank, addr + 1)?;
    Some(u16::from(lo) | (u16::from(hi) << 8))
}

/// The per-(area, stage 0) record: the palette `Y` selector and the four CHR
/// banks `$E0-$E3` **as stored** (before handle resolution).
struct Record {
    y: u8,
    chr: [u8; 4],
}

impl Record {
    fn decode(prg: &[u8], area: usize) -> Option<Self> {
        if area >= AREAS {
            return None;
        }
        let stage_table = read_word(prg, TABLE_BANK, AREA_RECORD_TABLE + area * 2)?;
        let record = read_word(prg, RECORD_BANK, usize::from(stage_table))?;
        let base = usize::from(record);
        Some(Self {
            y: read(prg, RECORD_BANK, base)?,
            chr: [
                read(prg, RECORD_BANK, base + 1)?,
                read(prg, RECORD_BANK, base + 2)?,
                read(prg, RECORD_BANK, base + 3)?,
                read(prg, RECORD_BANK, base + 4)?,
            ],
        })
    }
}

/// Resolve a stored CHR bank: values below `$80` are literal; a value with bit
/// 7 set is a handle whose group record's last byte is the real bank.
fn resolve_chr(prg: &[u8], handle: u8) -> Option<u8> {
    if handle < 0x80 {
        return Some(handle);
    }
    let index = (usize::from(handle) << 1) & 0x7F;
    let group = read_word(prg, TABLE_BANK, GROUP_TABLE + index)?;
    let count = read(prg, RECORD_BANK, usize::from(group))?;
    read(
        prg,
        RECORD_BANK,
        usize::from(group) + 1 + usize::from(count),
    )
}

/// A background CHR slot the game animates: the same handle record the CHR
/// loader reads, cycled by `far_D2B0` (bank 14 `$D2B0`).
///
/// `period` is the game's `$0430` (frames each bank is shown), `banks` its
/// `$0410` sequence (filled from the group record's bytes 2..) and `count` the
/// `$0431` sequence length (`<= 8`, the size of `$0410`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundAnimation {
    /// Frames each bank is shown.
    pub period: u8,
    /// The sequence, `count` entries long.
    pub banks: [u8; 8],
    /// Sequence length.
    pub count: u8,
}

/// The background CHR animations for a level's four `$E0-$E3` slots, decoded
/// from the PRG ROM.
///
/// Slot `i` is `Some` exactly when the record's byte `i` is a handle
/// (`>= $80`): `sub_B40A` sets bit `i` of `$0401`, and `far_D2B0` animates the
/// slot. The handle's group record is the same one [`resolve_chr`] reads
/// (`$C06C[(handle << 1) & $7F]`): byte 0 is the sequence length, byte 1 the
/// period, bytes 2.. the banks. `area` is the game's `$0400 / 2`.
#[must_use]
pub fn background_animation_for_area(
    prg: &[u8],
    area: usize,
) -> Option<[Option<BackgroundAnimation>; 4]> {
    let record = Record::decode(prg, area)?;
    let mut animations = [None; 4];
    for (slot, &stored) in record.chr.iter().enumerate() {
        if stored < 0x80 {
            continue;
        }
        let index = (usize::from(stored) << 1) & 0x7F;
        let group = usize::from(read_word(prg, TABLE_BANK, GROUP_TABLE + index)?);
        let count = read(prg, RECORD_BANK, group)?;
        // `$0410` holds at most 8 bytes per slot, and a count of 0 has no
        // sequence to cycle; both cases would break the wrap in `far_D2B0`.
        if !(1..=8).contains(&count) {
            continue;
        }
        let period = read(prg, RECORD_BANK, group + 1)?;
        let mut banks = [0u8; 8];
        for (j, bank) in banks.iter_mut().enumerate().take(usize::from(count)) {
            *bank = read(prg, RECORD_BANK, group + 2 + j)?;
        }
        animations[slot] = Some(BackgroundAnimation {
            period,
            banks,
            count,
        });
    }
    Some(animations)
}

/// A level's background CHR animations through the `$20` -> area table.
#[must_use]
pub fn background_animation_for_level(
    prg: &[u8],
    level: usize,
) -> Option<[Option<BackgroundAnimation>; 4]> {
    background_animation_for_area(prg, crate::levelmap::area_for_level(level)?)
}

/// The palette `X` selector for an area at stage 0 (bank 13 `$B5F5`).
fn x_selector(prg: &[u8], area: usize) -> Option<u8> {
    let table = read_word(prg, XSELECT_BANK, XSELECT_TABLE + area * 2)?;
    read(prg, XSELECT_BANK, usize::from(table))
}

/// `far_C367`: the base palette for the selectors `X` and `Y`, colour 0 of each
/// palette forced to `$0F` by `sub_D481`.
fn base_palette(prg: &[u8], x_sel: u8, y_sel: u8) -> Option<[u8; PALETTE_BYTES]> {
    let mut palette = [0x0F; PALETTE_BYTES];
    for step in 0..8 {
        // The loop runs x = $0E, $0C, ... $00; x >= 8 uses X, x < 8 uses Y.
        let x = 0x0E - 2 * step;
        let pointer = read_word(prg, TABLE_BANK, PALETTE_POINTER_TABLE + x)?;
        let selector = if x >= 8 { x_sel } else { y_sel };
        let index = usize::from(read(
            prg,
            TABLE_BANK,
            usize::from(pointer) + usize::from(selector),
        )?);
        let at = 2 * x;
        for (channel, table) in PALETTE_COLOR_TABLES.iter().enumerate() {
            palette[at + 1 + channel] = read(prg, TABLE_BANK, table + index)?;
        }
    }
    Some(palette)
}

/// The 32 palette-RAM bytes (`$3F00-$3F1F`) a level settles on during gameplay,
/// decoded from the PRG ROM. `area` is the game's `$0400 / 2`.
///
/// Returns `None` when the ROM is too short or `area` has no record.
#[must_use]
pub fn palette_for_area(prg: &[u8], area: usize) -> Option<[u8; PALETTE_BYTES]> {
    let record = Record::decode(prg, area)?;
    base_palette(prg, x_selector(prg, area)?, record.y)
}

/// The FME-7 CHR 1 KiB banks (commands 0-7, shadow `$88-$8F`) a level runs with,
/// decoded from the PRG ROM. `area` is the game's `$0400 / 2`.
///
/// Returns `None` when the ROM is too short or `area` has no record.
#[must_use]
pub fn chr_banks_for_area(prg: &[u8], area: usize) -> Option<[u8; CHR_BANKS]> {
    let record = Record::decode(prg, area)?;
    Some([
        resolve_chr(prg, record.chr[0])?,
        resolve_chr(prg, record.chr[1])?,
        resolve_chr(prg, record.chr[2])?,
        resolve_chr(prg, record.chr[3])?,
        CHR_FIXED[0],
        CHR_FIXED[1],
        CHR_FIXED[2],
        CHR_FIXED[3],
    ])
}

/// A level's palette through the level index `$20` -> area table
/// ([`crate::levelmap::area_for_level`]).
#[must_use]
pub fn palette_for_level(prg: &[u8], level: usize) -> Option<[u8; PALETTE_BYTES]> {
    palette_for_area(prg, crate::levelmap::area_for_level(level)?)
}

/// A level's CHR-bank set through the level index `$20` -> area table.
#[must_use]
pub fn chr_banks_for_level(prg: &[u8], level: usize) -> Option<[u8; CHR_BANKS]> {
    chr_banks_for_area(prg, crate::levelmap::area_for_level(level)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::actor::oracle::Scene;

    /// The `digests.txt` entry `name`, unit `-`.
    fn digest(name: &str) -> u64 {
        crate::digest::table(include_str!("../tests/data/digests.txt"), name)["-"]
    }

    /// The oracle's settled gameplay state for `level`: its frame-624 CHR set
    /// `$E0-$E7` and its frame-744 palette `$3F00-$3F1F`, from one power-on run. The
    /// palette is read with the PPU's colour mirrors (`$3F10-$3F1C` read `$3F00-$3F0C`)
    /// the captured dump holds, and the CHR set before sprite animation swaps `$E5`.
    /// Each level is pinned to its `stagearea-probe` capture by `digests.txt`.
    fn captured_level(level: usize) -> Option<([u8; PALETTE_BYTES], [u8; CHR_BANKS])> {
        let mut scene = Scene::level(u8::try_from(level).expect("13 levels"), 623)?;
        let chr: [u8; CHR_BANKS] = scene.machine.bus.ram[0xE0..0xE8]
            .try_into()
            .expect("8 CHR banks");
        assert_eq!(
            crate::digest::fnv1a(&chr),
            digest(&format!("level-chr-{level}")),
            "level {level}: the oracle's frame-624 CHR set differs from its capture"
        );
        scene.run(743 - 623, &[]);
        let mut palette = scene.machine.bus.ppu.palette;
        for i in 0..4 {
            palette[0x10 + i * 4] = palette[i * 4];
        }
        assert_eq!(
            crate::digest::fnv1a(&palette),
            digest(&format!("level-palette-{level}")),
            "level {level}: the oracle's frame-744 palette differs from its capture"
        );
        Some((palette, chr))
    }

    #[test]
    fn rejects_a_short_rom() {
        assert!(palette_for_area(&[0u8; 16], 0).is_none());
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        assert!(palette_for_area(&rom.prg, 18).is_none());
    }

    /// Acceptance: for every level with a capture, the decoded palette equals
    /// the oracle's `$3F00-$3F1F` at frame 744 (settled state) and the decoded CHR
    /// set equals its mapper state (`$E0-$E7` at frame 624).
    #[test]
    fn decoded_palette_and_chr_match_every_captured_level() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        let mut failures = 0;
        for level in 0..13 {
            let Some((captured_palette, captured_chr)) = captured_level(level) else {
                return;
            };
            let palette = palette_for_level(prg, level).expect("level palette");
            let chr = chr_banks_for_level(prg, level).expect("level chr");
            let palette_diff = palette
                .iter()
                .zip(captured_palette)
                .filter(|(a, b)| a != &b)
                .count();
            let chr_diff = chr
                .iter()
                .zip(captured_chr)
                .filter(|(a, b)| a != &b)
                .count();
            println!(
                "-- level {level}: palette {palette_diff}/{PALETTE_BYTES} differ, \
                 chr {chr_diff}/{CHR_BANKS} differ"
            );
            if palette_diff > 0 {
                println!("   got {palette:02x?}");
                println!("   cap {captured_palette:02x?}");
            }
            failures += palette_diff + chr_diff;
        }
        assert_eq!(failures, 0, "a level differs from its capture");
    }

    /// Every area resolves, and the four stored CHR banks all go through handle
    /// resolution where the record says so (regression for the `$C06C` walk).
    #[test]
    fn resolves_every_levels_chr_handles() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        for level in 0..13 {
            let chr = chr_banks_for_level(&rom.prg, level).unwrap();
            // The four lower banks are the level's tiles, the upper four fixed.
            assert_eq!(&chr[4..], &CHR_FIXED);
        }
    }

    /// The animation records decoded from the ROM match the emulator's own
    /// `$0410`/`$0430-$0431`: stage 1-1 (`area 0`) animates only slot 3 (`$E3`)
    /// with period 2 and the sequence `30 31 .. 37` (oracle, `docs/LEVELDATA.md`
    /// §16; the same bytes are in the oracle's `$0428`/`$043C`).
    #[test]
    fn decodes_the_background_chr_animations() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let prg = &rom.prg;
        let area0 = background_animation_for_area(prg, 0).expect("area 0 animations");
        assert_eq!(area0[..3], [None, None, None], "only $E3 animates");
        assert_eq!(
            area0[3],
            Some(BackgroundAnimation {
                period: 2,
                count: 8,
                banks: [0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37],
            })
        );
        // The load-time bank the CHR loader resolves is the last bank shown.
        let chr = chr_banks_for_area(prg, 0).unwrap();
        assert_eq!(chr[3], 0x37);
    }
}
