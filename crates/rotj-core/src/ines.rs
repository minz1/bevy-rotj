//! iNES ROM file format parsing.

use thiserror::Error;

/// The four bytes every iNES file starts with (`NES\x1a`).
pub const INES_MAGIC: [u8; 4] = *b"NES\x1a";
/// 8 KiB, as CHR bank counts are unit-based in the header.
pub const CHR_BANK_SIZE: usize = 8 * 1024;
/// 16 KiB, as PRG bank sizes are unit-based in the header.
pub const PRG_BANK_SIZE: usize = 16 * 1024;
/// The FME-7 switches PRG in 8 KiB windows, so the ROM's 128 KiB of PRG is 16
/// of these — the unit `disasm/` and `MAP.md` call "bank".
pub const FME7_PRG_BANK_SIZE: usize = 8 * 1024;

/// Anything malformed about an iNES file.
#[derive(Debug, Error)]
pub enum InesError {
    /// Not enough bytes / not a file starting with `NES\x1a`.
    #[error("not an iNES file: expected magic {INES_MAGIC:02x?}")]
    Magic,
    /// PRG/CHR bank counts don't add up to the file length.
    #[error("file length {len} does not match header-declared {prg} PRG + {chr} CHR banks")]
    Truncated {
        /// Actual file length.
        len: usize,
        /// Header-declared PRG bank count.
        prg: usize,
        /// Header-declared CHR bank count.
        chr: usize,
    },
}

/// The 16 bytes of the iNES header (NES 2.0 not supported — ROTJ is plain iNES).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InesHeader {
    prg_banks: u8,
    chr_banks: u8,
    flags6: u8,
    flags7: u8,
}

impl InesHeader {
    /// Parses the 16-byte header out of the first bytes of an iNES file.
    ///
    /// # Errors
    /// [`InesError::Magic`] when the buffer is too short or lacks the magic.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InesError> {
        if bytes.len() < 16 || bytes[..4] != INES_MAGIC {
            return Err(InesError::Magic);
        }
        Ok(Self {
            prg_banks: bytes[4],
            chr_banks: bytes[5],
            flags6: bytes[6],
            flags7: bytes[7],
        })
    }

    /// Number of 16 KiB PRG banks declared by the header.
    #[must_use]
    pub fn prg_banks(self) -> u8 {
        self.prg_banks
    }

    /// Number of 8 KiB CHR banks declared by the header.
    #[must_use]
    pub fn chr_banks(self) -> u8 {
        self.chr_banks
    }

    /// iNES mapper number (flags 7 high nibble | flags 6 high nibble).
    #[must_use]
    pub fn mapper(self) -> u8 {
        (self.flags7 & 0xf0) | (self.flags6 >> 4)
    }

    /// Whether the header flags battery-backed PRG RAM.
    #[must_use]
    pub fn has_battery(self) -> bool {
        self.flags6 & 0x10 != 0
    }

    /// Whether the header requests vertical mirroring; mapper 69 carts switch
    /// mirroring dynamically, so this is informational only.
    #[must_use]
    pub fn mirroring_vertical(self) -> bool {
        // Mapper 69 ignores the cart mirroring bits (dynamic), but parse anyway.
        self.flags6 & 0x02 != 0
    }
}

/// A parsed iNES ROM split into PRG and CHR sections.
pub struct InesRom {
    header: InesHeader,
    prg: Vec<u8>,
    chr: Vec<u8>,
}

impl InesRom {
    /// Parse a whole iNES file into header + PRG + CHR slices.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, InesError> {
        let header = InesHeader::from_bytes(bytes)?;
        let prg = header.prg_banks as usize * PRG_BANK_SIZE;
        let chr = header.chr_banks as usize * CHR_BANK_SIZE;
        let needed = 16 + prg + chr;
        if bytes.len() != needed {
            return Err(InesError::Truncated {
                len: bytes.len(),
                prg,
                chr,
            });
        }
        Ok(Self {
            header,
            prg: bytes[16..16 + prg].to_vec(),
            chr: bytes[16 + prg..].to_vec(),
        })
    }

    #[must_use]
    /// The parsed header of this ROM.
    pub fn header(&self) -> InesHeader {
        self.header
    }

    /// All PRG banks as `[u8; PRG_BANK_SIZE]` arrays.
    pub fn prg_banks(&self) -> impl Iterator<Item = &[u8; PRG_BANK_SIZE]> {
        self.prg.as_chunks::<PRG_BANK_SIZE>().0.iter()
    }

    /// A PRG 16 KiB bank by index, if in range.
    #[must_use]
    pub fn prg_bank(&self, index: usize) -> Option<&[u8]> {
        if index < self.header.prg_banks as usize {
            Some(&self.prg[index * PRG_BANK_SIZE..][..PRG_BANK_SIZE])
        } else {
            None
        }
    }

    /// All CHR banks (8 KiB each) as arrays.
    pub fn chr_banks(&self) -> impl Iterator<Item = &[u8; CHR_BANK_SIZE]> {
        self.chr.as_chunks::<CHR_BANK_SIZE>().0.iter()
    }

    /// A CHR 8 KiB bank by index, if in range.
    #[must_use]
    pub fn chr_bank(&self, index: usize) -> Option<&[u8]> {
        if index < self.header.chr_banks as usize {
            Some(&self.chr[index * CHR_BANK_SIZE..][..CHR_BANK_SIZE])
        } else {
            None
        }
    }
}

/// Neutral grayscale ramp mapping the four 2-bit tile color indices to
/// brightness. Used for CHR bank previews until scene palettes are recovered
/// from the disassembly (true NES colors need per-scene PPU palette RAM).
pub const GRAY_PREVIEW_PALETTE: [u8; 4] = [0x00, 0x55, 0xaa, 0xff];
