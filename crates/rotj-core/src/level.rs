//! Level data: what the ROM holds for a stage/sublevel, decoded.
//!
//! This module decodes the parts of the level format that are **proven** by the
//! disassembly (`disasm/MAP.md` §4/§5, `disasm/bank02.s`), and is explicit about
//! the parts that are not yet decoded (see `docs/LEVELDATA.md`).
//!
//! # The format, as proven
//!
//! The bank-02 loader at CPU `$8F2B` (bank 02 offset `$0F2B`) runs after a
//! sublevel is selected. It does three things:
//!
//! 1. **Nametable patch.** It walks a PPU write-list at CPU `$91B7` (bank 02
//!    offset `$11B7`) and copies each entry straight to `$2006`/`$2007`. The
//!    list uses the same `[hi, lo, count, bytes...]` `$FF`-terminated format
//!    [`crate::ppulist`] already decodes; the loader stops on a high byte with
//!    bit 7 set (`BMI`, `$8F2E`). A second list at `$925A` is applied later in
//!    the same routine.
//! 2. **Collision bitmap.** From `$8F4C` it reads 80 bytes at CPU `$9204`
//!    (bank 02 offset `$1204`), and for each byte expands its 8 bits **MSB
//!    first** into 8 bytes of `$00` (clear) or `$11` (set), writing 10 chunks
//!    of 64 bytes to RAM `$6000` (640 bytes, `$6000-$627F`).
//! 3. **A third list** at CPU `$93D7` is used by the sibling loader `$92AD`.
//!
//! Beyond these, the sublevel's *visible* tilemap is streamed to the nametable
//! by the renderers at `$8000`/`$8162`, which translate a RAM metatile map
//! (`$6000`/`$6E00`) through four 256-byte lookup tables (`$7000`, `$70D0`,
//! `$71A0`, `$7270`) that are themselves generated per area from a compact
//! CHR-resident definition. That generator is **not** decoded here — see
//! `docs/LEVELDATA.md` for the evidence and the open questions.

use thiserror::Error;

use crate::ines::FME7_PRG_BANK_SIZE;
use crate::ppulist::WriteList;

/// PRG bank holding the level loader and its tables.
pub const LEVEL_BANK: u8 = 0x02;

/// CPU `$91B7` = bank [`LEVEL_BANK`] offset `$11B7`: the loader's nametable
/// write-list (`disasm/MAP.md` §4, `disasm/bank02.s` `tbl_91b7_ppulist`).
pub const NAMETABLE_LIST: u16 = 0x11B7;

/// CPU `$925A` = bank [`LEVEL_BANK`] offset `$125A`: a second write-list applied
/// by the same `$8F2B` loader.
pub const NAMETABLE_LIST2: u16 = 0x125A;

/// CPU `$9204` = bank [`LEVEL_BANK`] offset `$1204`: the bit-packed source the
/// loader expands into RAM `$6000-$627F`.
///
/// Corrections (`docs/LEVELDATA.md` §8): the 80 source bytes are ten 8x8
/// **glyph rows** (the digits 0-9), so the expansion is a 1-bpp tile pattern
/// buffer, not a solidity map; and the routine that builds it (`$8F4C`) is the
/// sublevel intro/restart path (it ends `JMP ($FFFC)`, the RESET vector). The
/// name is kept for API stability.
pub const COLLISION_SOURCE: u16 = 0x1204;

/// CPU `$93D7` = bank [`LEVEL_BANK`] offset `$13D7`: the write-list used by the
/// sibling screen loader at `$92AD`.
pub const SCREEN_LIST: u16 = 0x13D7;

/// Bytes read from [`COLLISION_SOURCE`]: 10 chunks x 8 rows x 1 byte.
pub const COLLISION_SOURCE_BYTES: usize = 80;

/// Size of the expanded collision bitmap the loader writes to RAM `$6000`.
pub const COLLISION_BITMAP_BYTES: usize = 640;

/// The byte the loader writes for a set collision bit (`LDA #$11`, `$8F74`).
pub const COLLISION_SET: u8 = 0x11;

/// Both physical nametables together (`$2000-$27FF`).
pub const NAMETABLES_BYTES: usize = 0x0800;

/// A stage/sublevel identifier.
///
/// The running game keeps these in RAM: `$0401` = sublevel, `$0402` = stage
/// (frame 700 of stage 1-1 reads `$0401 = 8`, `$0402 = 0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SublevelId {
    /// Stage index (RAM `$0402`), 0-based.
    pub stage: u8,
    /// Sublevel index within the stage (RAM `$0401`), 0-based.
    pub sublevel: u8,
}

/// One nametable/attribute patch: a decoded PPU write-list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// PPU destination, e.g. `$20EB` (nametable) or `$23D0` (attributes).
    pub addr: u16,
    /// Bytes written starting at [`Patch::addr`].
    pub data: Vec<u8>,
}

impl Patch {
    /// Overlay this patch onto a two-physical-nametable image (`$2000-$27FF`).
    ///
    /// Bytes that fall outside the image are ignored, matching the PPU wrap
    /// behaviour closely enough for the level's own patches (all in range).
    pub fn apply(&self, nametables: &mut [u8]) {
        let base = usize::from(self.addr.wrapping_sub(0x2000));
        for (i, byte) in self.data.iter().enumerate() {
            if let Some(slot) = nametables.get_mut(base + i) {
                *slot = *byte;
            }
        }
    }
}

/// Everything the ROM gives up for one sublevel at this level of decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelData {
    /// Which sublevel this was decoded for.
    pub id: SublevelId,
    /// The nametable patches the loader applies, in list order.
    pub patches: Vec<Patch>,
    /// The expanded RAM `$6000` collision bitmap ([`COLLISION_BITMAP_BYTES`]).
    pub collision: Vec<u8>,
}

/// A ROM address was outside the PRG data.
#[derive(Debug, Error)]
pub enum LevelError {
    /// A bank/offset pair pointed past the end of `prg`.
    #[error("bank {bank:02X} offset ${offset:04X} is outside PRG ({len} bytes)")]
    OutOfRange {
        /// FME-7 PRG bank number.
        bank: u8,
        /// Offset inside that bank.
        offset: u16,
        /// Length of the PRG slice that was searched.
        len: usize,
    },
}

/// The `bank`/`offset` slice of `prg`, or [`LevelError::OutOfRange`].
fn bank_slice(prg: &[u8], bank: u8, offset: u16) -> Result<&[u8], LevelError> {
    let base = usize::from(bank) * FME7_PRG_BANK_SIZE + usize::from(offset);
    prg.get(base..).ok_or(LevelError::OutOfRange {
        bank,
        offset,
        len: prg.len(),
    })
}

/// Decode one `$FF`-terminated PPU write-list into its patches.
///
/// # Errors
/// [`LevelError::OutOfRange`] if the list's base address is outside `prg`.
pub fn decode_ppu_list(prg: &[u8], bank: u8, offset: u16) -> Result<Vec<Patch>, LevelError> {
    let slice = bank_slice(prg, bank, offset)?;
    Ok(WriteList::new(slice)
        .map(|entry| Patch {
            addr: entry.addr,
            data: entry.data.to_vec(),
        })
        .collect())
}

/// Expand the bit-packed [`COLLISION_SOURCE`] table into RAM `$6000-$627F`,
/// exactly as the routine at `$8F4C` does. **Not** a solidity map — see
/// [`COLLISION_SOURCE`] and `docs/LEVELDATA.md` §8.
///
/// Each source byte becomes 8 output bytes, most-significant bit first: a set
/// bit yields [`COLLISION_SET`], a clear bit `$00`. The 10 chunks of 8 source
/// bytes each land 64 bytes apart, so the result is
/// [`COLLISION_BITMAP_BYTES`] long.
///
/// # Errors
/// [`LevelError::OutOfRange`] if the source table is outside `prg`.
pub fn collision_bitmap(prg: &[u8]) -> Result<Vec<u8>, LevelError> {
    let slice = bank_slice(prg, LEVEL_BANK, COLLISION_SOURCE)?;
    let source = slice
        .get(..COLLISION_SOURCE_BYTES)
        .ok_or(LevelError::OutOfRange {
            bank: LEVEL_BANK,
            offset: COLLISION_SOURCE,
            len: prg.len(),
        })?;
    let mut out = Vec::with_capacity(COLLISION_BITMAP_BYTES);
    for &byte in source {
        for bit in (0..8).rev() {
            out.push(if byte >> bit & 1 == 1 {
                COLLISION_SET
            } else {
                0
            });
        }
    }
    Ok(out)
}

impl LevelData {
    /// Decode everything this module knows for `id`.
    ///
    /// # Errors
    /// [`LevelError::OutOfRange`] if the ROM's level tables are outside `prg`.
    pub fn decode(prg: &[u8], id: SublevelId) -> Result<Self, LevelError> {
        let mut patches = decode_ppu_list(prg, LEVEL_BANK, NAMETABLE_LIST)?;
        patches.extend(decode_ppu_list(prg, LEVEL_BANK, NAMETABLE_LIST2)?);
        Ok(Self {
            id,
            patches,
            collision: collision_bitmap(prg)?,
        })
    }

    /// Render the decoded patches over a blank 2 KiB nametable image.
    #[must_use]
    pub fn nametable_image(&self) -> Vec<u8> {
        let mut image = vec![0u8; NAMETABLES_BYTES];
        for patch in &self.patches {
            patch.apply(&mut image);
        }
        image
    }

    /// How many of the decoded patch bytes land in `nametables` with the same
    /// value, as `(matches, total)`. This isolates the patch from the streamed
    /// background: it is the only part of the capture the ROM lists can explain.
    #[must_use]
    pub fn patch_agreement(&self, nametables: &[u8]) -> (usize, usize) {
        let mut matches = 0;
        let mut total = 0;
        for patch in &self.patches {
            let base = usize::from(patch.addr.wrapping_sub(0x2000));
            for (i, byte) in patch.data.iter().enumerate() {
                if let Some(&captured) = nametables.get(base + i) {
                    total += 1;
                    if captured == *byte {
                        matches += 1;
                    }
                }
            }
        }
        (matches, total)
    }
}

/// Byte-level diff of two images: `(mismatched, compared)`, compared up to the
/// shorter length.
#[must_use]
pub fn diff_bytes(a: &[u8], b: &[u8]) -> (usize, usize) {
    let n = a.len().min(b.len());
    let mismatched = a[..n].iter().zip(&b[..n]).filter(|(x, y)| x != y).count();
    (mismatched, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PRG holding one write-list and the collision table.
    fn synthetic_prg() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * FME7_PRG_BANK_SIZE];
        let base = usize::from(LEVEL_BANK) * FME7_PRG_BANK_SIZE;
        // Nametable list at $11B7: one entry, $2100 <- AA BB, then $FF.
        prg[base + usize::from(NAMETABLE_LIST)] = 0x21;
        prg[base + usize::from(NAMETABLE_LIST) + 1] = 0x00;
        prg[base + usize::from(NAMETABLE_LIST) + 2] = 0x02;
        prg[base + usize::from(NAMETABLE_LIST) + 3] = 0xAA;
        prg[base + usize::from(NAMETABLE_LIST) + 4] = 0xBB;
        prg[base + usize::from(NAMETABLE_LIST) + 5] = 0xFF;
        // Second list at $125A: empty (immediate $FF).
        prg[base + usize::from(NAMETABLE_LIST2)] = 0xFF;
        // Collision table at $1204: 80 bytes, first is $80, second $01.
        prg[base + usize::from(COLLISION_SOURCE)] = 0x80;
        prg[base + usize::from(COLLISION_SOURCE) + 1] = 0x01;
        prg
    }

    #[test]
    fn decodes_a_ppu_list_into_patches() {
        let prg = synthetic_prg();
        let patches = decode_ppu_list(&prg, LEVEL_BANK, NAMETABLE_LIST).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].addr, 0x2100);
        assert_eq!(patches[0].data, [0xAA, 0xBB]);
    }

    #[test]
    fn collision_bitmap_expands_msb_first() {
        let prg = synthetic_prg();
        let bitmap = collision_bitmap(&prg).unwrap();
        assert_eq!(bitmap.len(), COLLISION_BITMAP_BYTES);
        // $80 -> first output byte set, next seven clear.
        assert_eq!(&bitmap[..8], &[COLLISION_SET, 0, 0, 0, 0, 0, 0, 0]);
        // $01 -> first seven clear, last set.
        assert_eq!(&bitmap[8..16], &[0, 0, 0, 0, 0, 0, 0, COLLISION_SET]);
    }

    #[test]
    fn patch_apply_overlays_and_clamps() {
        let patch = Patch {
            addr: 0x2000,
            data: vec![1, 2, 3],
        };
        let mut image = vec![0u8; NAMETABLES_BYTES];
        patch.apply(&mut image);
        assert_eq!(&image[..4], &[1, 2, 3, 0]);
        // An out-of-range patch writes nothing rather than panicking.
        let patch = Patch {
            addr: 0x27FE,
            data: vec![9, 9, 9],
        };
        patch.apply(&mut image);
        assert_eq!(image[NAMETABLES_BYTES - 2], 9);
    }

    #[test]
    fn decode_collects_both_lists_and_collision() {
        let prg = synthetic_prg();
        let level = LevelData::decode(
            &prg,
            SublevelId {
                stage: 0,
                sublevel: 8,
            },
        )
        .unwrap();
        assert_eq!(level.patches.len(), 1);
        assert_eq!(level.collision.len(), COLLISION_BITMAP_BYTES);
        let image = level.nametable_image();
        assert_eq!(image[0x100], 0xAA);
        assert_eq!(image[0x101], 0xBB);
    }

    #[test]
    fn patch_agreement_counts_matches() {
        let prg = synthetic_prg();
        let level = LevelData::decode(
            &prg,
            SublevelId {
                stage: 0,
                sublevel: 0,
            },
        )
        .unwrap();
        let mut captured = vec![0u8; NAMETABLES_BYTES];
        captured[0x100] = 0xAA; // one of the two patch bytes agrees
        assert_eq!(level.patch_agreement(&captured), (1, 2));
    }

    #[test]
    fn out_of_range_is_an_error_not_a_panic() {
        let prg = vec![0u8; 16];
        assert!(matches!(
            decode_ppu_list(&prg, LEVEL_BANK, NAMETABLE_LIST),
            Err(LevelError::OutOfRange { .. })
        ));
        assert!(matches!(
            collision_bitmap(&prg),
            Err(LevelError::OutOfRange { .. })
        ));
    }

    #[test]
    fn diff_counts_mismatches() {
        assert_eq!(diff_bytes(&[1, 2, 3], &[1, 9, 3]), (1, 3));
        assert_eq!(diff_bytes(&[1, 2], &[1, 2, 3]), (0, 2));
    }
}
