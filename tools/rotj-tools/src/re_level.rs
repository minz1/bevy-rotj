//! Level data: what the ROM holds for a stage/sublevel, decoded (RE tooling).

use rotj_core::ppulist::WriteList;
use rotj_core::render::ppu;
use thiserror::Error;

/// PRG bank holding the level loader and its tables.
pub const LEVEL_BANK: u8 = 0x02;

/// CPU `$91B7` = bank [`LEVEL_BANK`] offset `$11B7`: the loader's nametable write-list.
pub const NAMETABLE_LIST: u16 = 0x11B7;

/// CPU `$925A` = bank [`LEVEL_BANK`] offset `$125A`: a second write-list applied by `$8F2B`.
pub const NAMETABLE_LIST2: u16 = 0x125A;

/// CPU `$9204` = bank [`LEVEL_BANK`] offset `$1204`: the bit-packed source the loader expands.
pub const COLLISION_SOURCE: u16 = 0x1204;

/// Bytes read from [`COLLISION_SOURCE`]: 10 chunks x 8 rows x 1 byte.
pub const COLLISION_SOURCE_BYTES: usize = 80;

/// Size of the expanded bitmap the loader writes to RAM `$6000`.
pub const COLLISION_BITMAP_BYTES: usize = 640;

/// The byte the loader writes for a set bit (`LDA #$11`, `$8F74`).
pub const COLLISION_SET: u8 = 0x11;

/// Both physical nametables together (`$2000-$27FF`).
pub const NAMETABLES_BYTES: usize = 0x0800;

/// A stage/sublevel identifier (`$0402` / `$0401`).
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
    pub fn apply(&self, nametables: &mut [u8]) {
        let base = usize::from(self.addr.wrapping_sub(ppu::nametable(0)));
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
    /// The nametable patches the loader applies, in list order.
    pub patches: Vec<Patch>,
    /// The expanded RAM `$6000` bitmap ([`COLLISION_BITMAP_BYTES`]).
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

fn bank_slice(prg: &[u8], bank: u8, offset: u16) -> Result<&[u8], LevelError> {
    rotj_core::prg::from_bank(prg, usize::from(bank), usize::from(offset)).ok_or(
        LevelError::OutOfRange {
            bank,
            offset,
            len: prg.len(),
        },
    )
}

/// Decode one `$FF`-terminated PPU write-list into its patches.
pub fn decode_ppu_list(prg: &[u8], bank: u8, offset: u16) -> Result<Vec<Patch>, LevelError> {
    let slice = bank_slice(prg, bank, offset)?;
    Ok(WriteList::new(slice)
        .map(|entry| Patch {
            addr: entry.addr,
            data: entry.data.to_vec(),
        })
        .collect())
}

/// Expand the bit-packed [`COLLISION_SOURCE`] table into RAM `$6000-$627F`.
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
    /// Decode both write-lists and the expanded bitmap.
    pub fn decode(prg: &[u8]) -> Result<Self, LevelError> {
        let mut patches = decode_ppu_list(prg, LEVEL_BANK, NAMETABLE_LIST)?;
        patches.extend(decode_ppu_list(prg, LEVEL_BANK, NAMETABLE_LIST2)?);
        Ok(Self {
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

    /// How many of the decoded patch bytes land in `nametables` with the same value.
    #[must_use]
    pub fn patch_agreement(&self, nametables: &[u8]) -> (usize, usize) {
        let mut matches = 0;
        let mut total = 0;
        for patch in &self.patches {
            let base = usize::from(patch.addr.wrapping_sub(ppu::nametable(0)));
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

/// Byte-level diff of two images: `(mismatched, compared)`, compared up to the shorter length.
#[must_use]
pub fn diff_bytes(a: &[u8], b: &[u8]) -> (usize, usize) {
    let n = a.len().min(b.len());
    let mismatched = a[..n].iter().zip(&b[..n]).filter(|(x, y)| x != y).count();
    (mismatched, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg() -> Vec<u8> {
        let mut prg = vec![0u8; 16 * rotj_core::prg::BANK_BYTES];
        let base = usize::from(LEVEL_BANK) * rotj_core::prg::BANK_BYTES;
        prg[base + usize::from(NAMETABLE_LIST)] = 0x21;
        prg[base + usize::from(NAMETABLE_LIST) + 1] = 0x00;
        prg[base + usize::from(NAMETABLE_LIST) + 2] = 0x02;
        prg[base + usize::from(NAMETABLE_LIST) + 3] = 0xAA;
        prg[base + usize::from(NAMETABLE_LIST) + 4] = 0xBB;
        prg[base + usize::from(NAMETABLE_LIST) + 5] = 0xFF;
        prg[base + usize::from(NAMETABLE_LIST2)] = 0xFF;
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
        assert_eq!(&bitmap[..8], &[COLLISION_SET, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&bitmap[8..16], &[0, 0, 0, 0, 0, 0, 0, COLLISION_SET]);
    }

    #[test]
    fn decode_collects_both_lists_and_collision() {
        let prg = synthetic_prg();
        let level = LevelData::decode(&prg).unwrap();
        assert_eq!(level.patches.len(), 1);
        assert_eq!(level.collision.len(), COLLISION_BITMAP_BYTES);
        let image = level.nametable_image();
        assert_eq!(image[0x100], 0xAA);
        assert_eq!(image[0x101], 0xBB);
        let mut captured = vec![0u8; NAMETABLES_BYTES];
        captured[0x100] = 0xAA;
        assert_eq!(level.patch_agreement(&captured), (1, 2));
        assert_eq!(diff_bytes(&[1, 2, 3], &[1, 9, 3]), (1, 3));
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
}
