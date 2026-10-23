//! CHR (PPU pattern table) tile decoding.

/// Tiles are 8x8 pixels at 2 bits per pixel, 16 bytes per tile (planes of 8
/// bytes, then high plane bytes).
pub const TILE_BYTES: usize = 16;
/// 8 KiB CHR bank.
pub const CHR_BANK_BYTES: usize = 8 * 1024;
/// A bank's worth of 8x8 tiles.
pub const TILES_PER_BANK: usize = CHR_BANK_BYTES / TILE_BYTES;
/// Tiles per row of the rendered bank sheet.
pub const TILE_SHEET_COLS: usize = 16;
/// Pixel width of the rendered bank sheet (16 tiles * 8 px).
pub const TILE_SHEET_W: usize = TILE_SHEET_COLS * 8;
/// Pixel height of the rendered bank sheet (32 tile rows * 8 px).
pub const TILE_SHEET_H: usize = (TILES_PER_BANK / TILE_SHEET_COLS) * 8;

/// 2-bit color index (`0..=3`) of pixel (`x`, `y`) within one 16-byte tile
/// pattern. Pixel (0, 0) is top-left.
#[must_use]
pub fn pixel(tile: &[u8], x: usize, y: usize) -> u8 {
    debug_assert_eq!(tile.len(), TILE_BYTES);
    let lo = (tile[y] >> (7 - x)) & 1; // first plane is LSB, second plane is MSB
    let hi = (tile[8 + y] >> (7 - x)) & 1;
    (hi << 1) | lo
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

/// Decode one 8x8 2bpp tile into `out` (RGBA), `stride` bytes per row.
fn blit_tile(tile: &[u8], out: &mut [u8], stride: usize, palette: [u8; 4]) {
    debug_assert_eq!(tile.len(), TILE_BYTES);
    for row in 0..8usize {
        for col in 0..8usize {
            let idx = usize::from(pixel(tile, col, row));
            let px = &mut out[row * stride + col * 4..][..4];
            px[0..3].copy_from_slice(&[palette[idx], palette[idx], palette[idx]]);
            px[3] = 0xff;
        }
    }
}

/// Render a whole 8 KiB CHR bank into an RGBA image of dimensions
/// [`TILE_SHEET_W`] x [`TILE_SHEET_H`].
#[must_use]
pub fn bank_to_rgba(bank: &[u8], palette: [u8; 4]) -> Vec<u8> {
    assert_eq!(bank.len(), CHR_BANK_BYTES);
    let mut out = vec![0u8; TILE_SHEET_W * TILE_SHEET_H * 4];
    for (i, tile) in bank.as_chunks::<TILE_BYTES>().0.iter().enumerate() {
        let cx = (i % TILE_SHEET_COLS) * 8;
        let cy = (i / TILE_SHEET_COLS) * 8;
        let w4 = TILE_SHEET_W * 4;
        blit_tile(tile, &mut out[cy * w4 + cx * 4..], w4, palette);
    }
    out
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
        blit_tile(&tile, &mut out, 8 * 4, [10, 60, 160, 255]);
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
