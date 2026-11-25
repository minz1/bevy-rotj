//! CHR (PPU pattern table) tile decoding.

/// Tiles are 8x8 pixels at 2 bits per pixel, 16 bytes per tile (planes of 8
/// bytes, then high plane bytes).
pub const TILE_BYTES: usize = 16;

/// 2-bit color index (`0..=3`) at bit position `bit` (`0..=7`, where 7 is the
/// leftmost pixel) from a low and high bitplane byte.
#[must_use]
pub const fn plane_pixel(low: u8, high: u8, bit: usize) -> u8 {
    (((high >> bit) & 1) << 1) | ((low >> bit) & 1)
}

/// 2-bit color index (`0..=3`) of pixel (`x`, `y`) within one 16-byte tile
/// pattern. Pixel (0, 0) is top-left.
#[must_use]
pub fn pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    debug_assert_eq!(tile.len(), TILE_BYTES);
    plane_pixel(tile[y], tile[8 + y], 7 - x)
}

/// The offset of 1 KiB `bank` in `chr`, wrapped to the ROM's length.
fn bank_start(chr: &[u8], bank: u8) -> usize {
    usize::from(bank) * 1024 % chr.len()
}

/// The byte at `addr` in the pattern table `banks` picks from `chr`.
#[must_use]
pub fn byte(chr: &[u8], banks: &[u8], addr: usize) -> u8 {
    chr[bank_start(chr, banks[(addr >> 10) & 7]) + (addr & 0x3FF)]
}

/// The 16 bytes of the tile at `addr` in the pattern table `banks` picks from `chr`.
#[must_use]
pub fn tile<'a>(chr: &'a [u8], banks: &[u8], addr: usize) -> &'a [u8] {
    let at = bank_start(chr, banks[(addr >> 10) & 7]) + (addr & 0x3FF);
    &chr[at..at + TILE_BYTES]
}

/// Decode one 8x8 2bpp tile into `out` (RGBA), `stride` bytes per row, mapping
/// each 2-bit pixel value `0..=3` through `colours`.
pub fn blit_rgba(tile: &[u8], out: &mut [u8], stride: usize, colours: [[u8; 4]; 4]) {
    debug_assert_eq!(tile.len(), TILE_BYTES);
    for row in 0..8usize {
        for col in 0..8usize {
            let idx = usize::from(pixel(tile, col, row));
            out[row * stride + col * 4..][..4].copy_from_slice(&colours[idx]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_decodes_2bpp() {
        let mut tile = [0u8; TILE_BYTES];
        tile[0] = 0b1010_1010; // row 0 low plane
        tile[8] = 0b1100_1100; // row 0 high plane
        let mut out = [0u8; 8 * 8 * 4];
        blit_rgba(
            &tile,
            &mut out,
            8 * 4,
            [10, 60, 160, 255].map(|g| [g, g, g, 0xFF]),
        );
        // Row 0: hi=1100_1100, lo=1010_1010 → pixel pairs (hi,lo): (1,1)→3, (1,0)→2, (0,1)→1, (0,0)→0
        assert_eq!(
            out[0..16],
            [
                255, 255, 255, 255, // idx 3
                160, 160, 160, 255, // idx 2
                60, 60, 60, 255, // idx 1
                10, 10, 10, 255, // idx 0
            ]
        );
    }
}
