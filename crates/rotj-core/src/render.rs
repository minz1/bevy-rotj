//! Software renderer that reproduces the NES PPU's picture.
//!
//! The port draws every frame through this instead of Bevy's sprite batcher:
//! a 256x240 RGBA image built from the same inputs the PPU has (nametables,
//! pattern tables, palette RAM, OAM, scroll), so the output can be diffed
//! pixel-for-pixel against the hardware. Rendering happens per scanline band
//! because ROTJ swaps 1 KiB CHR banks mid-frame for parallax: the caller draws
//! each band with the `pattern` view that was live during it.

use std::ops::Range;

use crate::chr::{TILE_BYTES, tile_index};
use crate::palette;

/// Picture width in NES pixels.
pub const SCREEN_W: usize = 256;
/// Picture height in NES pixels.
pub const SCREEN_H: usize = 240;
/// Bytes in one RGBA frame.
pub const FRAME_BYTES: usize = SCREEN_W * SCREEN_H * 4;
/// One physical nametable: 32x30 tile indices + 64 attribute bytes.
pub const NAMETABLE_BYTES: usize = 1024;
/// Palette RAM size in bytes.
pub const PALETTE_BYTES: usize = 32;
/// Attribute table offset inside a nametable.
pub const ATTRIBUTE_OFFSET: usize = 960;
/// Palette-RAM offset of the sprite palettes (`$3F10`).
pub const SPRITE_PALETTE_OFFSET: usize = 0x10;
/// Most sprites the PPU can show on one scanline.
pub const MAX_SPRITES_PER_LINE: usize = 8;

/// How the four logical nametables fold onto the two physical ones, per iNES
/// header bit 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// `$2000`=`$2400` (table A), `$2800`=`$2C00` (table B): the table changes
    /// with Y. Used by vertically scrolling scenes.
    Horizontal,
    /// `$2000`=`$2800` (table A), `$2400`=`$2C00` (table B): the table changes
    /// with X. Most of ROTJ scrolls this way.
    Vertical,
}

/// Background-layer inputs, in NES units.
#[derive(Debug, Clone, Copy)]
pub struct Background<'a> {
    /// The two physical nametables, `[0]` = `$2000`, `[1]` = `$2400`.
    pub nametables: &'a [u8],
    /// Pattern bytes for the PPU's `$0000-$1FFF` range (8 KiB) as currently
    /// mapped by the cart's CHR banks.
    pub pattern: &'a [u8],
    /// `$0000` or `$1000`: background pattern table (PPUCTRL bit 4).
    pub pattern_base: usize,
    /// The 32 palette-RAM bytes.
    pub palette: &'a [u8; 32],
    /// Which physical nametable each quadrant of the scroll space reads from.
    pub mirroring: Mirroring,
    /// Horizontal scroll in pixels; wraps after 512 (two nametables).
    pub scroll_x: u16,
    /// Vertical scroll in pixels; wraps after 480 (two nametables).
    pub scroll_y: u16,
}

impl Background<'_> {
    fn tile_pattern(&self, tile: u8) -> &[u8] {
        let off = self.pattern_base + usize::from(tile) * TILE_BYTES;
        &self.pattern[off..off + TILE_BYTES]
    }
}

/// Sprite-layer inputs, in NES units.
#[derive(Debug, Clone, Copy)]
pub struct Sprites<'a> {
    /// The 256 OAM bytes: four per sprite (y, tile, attributes, x).
    pub oam: &'a [u8; 256],
    /// Pattern bytes for `$0000-$1FFF`, same convention as [`Background`].
    pub pattern: &'a [u8],
    /// PPUCTRL bit 3: `$1000` when set (8x8 sprites only).
    pub pattern_base: usize,
    /// PPUCTRL bit 5: 8x16 sprites.
    pub tall: bool,
    /// The 32 palette-RAM bytes; sprite palettes are at `$3F10-$3F1F`.
    pub palette: &'a [u8; 32],
}

/// A reusable frame buffer: RGBA output, the palette-RAM value behind every
/// pixel (for exact verification against emulator output), and the "background
/// is opaque here" mask sprite priority needs.
#[derive(Debug, Clone)]
pub struct Frame {
    rgba: Vec<u8>,
    indices: Vec<u8>,
    opaque: Vec<bool>,
}

impl Frame {
    /// A cleared frame.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rgba: vec![0; FRAME_BYTES],
            indices: vec![0; SCREEN_W * SCREEN_H],
            opaque: vec![false; SCREEN_W * SCREEN_H],
        }
    }

    /// Reset to fully transparent.
    pub fn clear(&mut self) {
        self.rgba.fill(0);
        self.indices.fill(0);
        self.opaque.fill(false);
    }

    /// The rendered RGBA bytes, `SCREEN_W * SCREEN_H * 4` of them.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The palette-RAM value shown at each pixel, in row-major order
    /// (`SCREEN_W * SCREEN_H` bytes, each `0..=63`). Two palette entries can
    /// map to the same RGB, so this is the unambiguous record of what was
    /// drawn — it is what verification diffing should compare.
    #[must_use]
    pub fn indices(&self) -> &[u8] {
        &self.indices
    }

    /// Draw background scanlines `band` (clamped to the screen).
    pub fn background(&mut self, band: Range<usize>, bg: &Background<'_>) {
        for y in clamp_band(band) {
            let my = (usize::from(bg.scroll_y) + y) % (2 * SCREEN_H);
            let py = my % SCREEN_H;
            let table_row = (my / SCREEN_H) & 1;
            for x in 0..SCREEN_W {
                let mx = (usize::from(bg.scroll_x) + x) % (2 * SCREEN_W);
                let px = mx % SCREEN_W;
                let table_col = (mx / SCREEN_W) & 1;
                let table = match bg.mirroring {
                    Mirroring::Vertical => table_col,
                    Mirroring::Horizontal => table_row,
                };
                let nt = &bg.nametables[table * NAMETABLE_BYTES..][..NAMETABLE_BYTES];
                let tile = nt[(py / 8) * 32 + px / 8];
                let attr = nt[ATTRIBUTE_OFFSET + (py / 32) * 8 + px / 32];
                // Attribute bits: 2 per 16x16 quadrant, top-left first.
                let quadrant = ((py % 32) / 16) * 2 + (px % 32) / 16;
                let sub_palette = usize::from(attr >> (quadrant * 2) & 3);
                let index = usize::from(tile_index(bg.tile_pattern(tile), px % 8, py % 8));
                // The destination is the *screen* column: `px` is the scrolled
                // source position, which only equals the screen column when the
                // scroll is zero.
                let dst = y * SCREEN_W + x;
                // Index 0 shows the universal background color ($3F00).
                let value = if index == 0 {
                    bg.palette[0]
                } else {
                    bg.palette[sub_palette * 4 + index]
                };
                self.opaque[dst] = index != 0;
                self.indices[dst] = value;
                self.rgba[dst * 4..dst * 4 + 4].copy_from_slice(&palette::rgba(value));
            }
        }
    }

    /// Draw sprites over the background for scanlines `band`, applying the
    /// PPU's per-scanline sprite limit and background-priority bit.
    pub fn sprites(&mut self, band: Range<usize>, sp: &Sprites<'_>) {
        let height = if sp.tall { 16 } else { 8 };
        for y in clamp_band(band) {
            let mut shown = 0;
            for i in 0..64 {
                let oam = &sp.oam[i * 4..i * 4 + 4];
                // OAM stores the sprite top minus one, so it survives y=0.
                let top = usize::from(oam[0]) + 1;
                if y < top || y >= top + height {
                    continue;
                }
                shown += 1;
                if shown > MAX_SPRITES_PER_LINE {
                    break;
                }
                let attr = oam[2];
                let flip_x = attr & 0x40 != 0;
                let flip_y = attr & 0x80 != 0;
                let row = y - top;
                let (table_base, tile, row_in) = if sp.tall {
                    // 8x16: tile bit 0 picks the pattern table, bits 1-7 the pair.
                    let r = if flip_y { 15 - row } else { row };
                    let base = if oam[1] & 1 != 0 { 0x1000 } else { 0x0000 };
                    (base, usize::from(oam[1] & 0xfe) + r / 8, r % 8)
                } else {
                    (
                        sp.pattern_base,
                        usize::from(oam[1]),
                        if flip_y { 7 - row } else { row },
                    )
                };
                let off = table_base + tile * TILE_BYTES;
                let pattern = &sp.pattern[off..off + TILE_BYTES];
                for col in 0..8 {
                    let src_x = if flip_x { 7 - col } else { col };
                    let index = usize::from(tile_index(pattern, src_x, row_in));
                    if index == 0 {
                        continue; // sprite pixel 0 is transparent
                    }
                    let px = (usize::from(oam[3]) + col) % SCREEN_W; // wrap at the edge
                    let dst = y * SCREEN_W + px;
                    if attr & 0x20 != 0 && self.opaque[dst] {
                        continue; // in front of the background only
                    }
                    let value =
                        sp.palette[SPRITE_PALETTE_OFFSET + usize::from(attr & 3) * 4 + index];
                    self.indices[dst] = value;
                    self.rgba[dst * 4..dst * 4 + 4].copy_from_slice(&palette::rgba(value));
                }
            }
        }
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

/// Clamp a band to the visible screen.
fn clamp_band(band: Range<usize>) -> Range<usize> {
    band.start.min(SCREEN_H)..band.end.min(SCREEN_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(frame: &Frame, x: usize, y: usize) -> [u8; 4] {
        let i = (y * SCREEN_W + x) * 4;
        <[u8; 4]>::try_from(&frame.rgba()[i..i + 4]).unwrap()
    }

    /// Pattern space where tile `n` is solid color `index`.
    fn solid_pattern(tile: u8, index: u8) -> Vec<u8> {
        let mut pattern = vec![0u8; 8192];
        let off = usize::from(tile) * TILE_BYTES;
        if index & 1 != 0 {
            pattern[off..off + 8].fill(0xff);
        }
        if index & 2 != 0 {
            pattern[off + 8..off + 16].fill(0xff);
        }
        pattern
    }

    fn two_nametables() -> Vec<u8> {
        vec![0u8; NAMETABLE_BYTES * 2]
    }

    #[test]
    fn background_attribute_selects_palette() {
        let mut nt = two_nametables();
        nt[0] = 1; // tile 1 at the top-left of nametable 0
        nt[ATTRIBUTE_OFFSET] = 0b0000_0010; // top-left quadrant -> palette 2
        let pattern = solid_pattern(1, 1);
        let mut palette = [0u8; 32];
        palette[0] = 0x0f;
        palette[2 * 4 + 1] = 0x21; // palette 2, color 1
        let frame = render(|f| {
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 0, 0));
        });
        assert_eq!(pixel(&frame, 0, 0), palette::rgba(0x21));
    }

    #[test]
    fn background_index_zero_uses_universal_color() {
        let nt = two_nametables(); // tile 0 everywhere, and tile 0 has no pixels
        let pattern = solid_pattern(1, 1);
        let mut palette = [0u8; 32];
        palette[0] = 0x0f;
        palette[1] = 0x21;
        let frame = render(|f| {
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 0, 0));
        });
        assert_eq!(pixel(&frame, 0, 0), palette::rgba(0x0f));
    }

    #[test]
    fn background_scroll_reaches_second_nametable() {
        let mut nt = two_nametables();
        nt[0] = 1; // nametable 0, top-left: color 1
        nt[NAMETABLE_BYTES] = 2; // nametable 1, top-left: color 2
        let mut pattern = solid_pattern(1, 1);
        pattern[2 * TILE_BYTES..2 * TILE_BYTES + 16]
            .copy_from_slice(&solid_pattern(0, 2)[0..TILE_BYTES]);
        let mut palette = [0u8; 32];
        palette[1] = 0x21;
        palette[2] = 0x11;
        // 256 pixels of horizontal scroll with vertical mirroring: the second
        // physical nametable takes over at x = 0.
        let frame = render(|f| {
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 256, 0));
        });
        assert_eq!(pixel(&frame, 0, 0), palette::rgba(0x11));
    }

    #[test]
    fn sprite_priority_bit_keeps_background_in_front() {
        let mut nt = two_nametables();
        nt[0] = 1;
        let mut pattern = solid_pattern(1, 1);
        // Sprite tile 2: solid color 1 too.
        pattern[2 * TILE_BYTES..2 * TILE_BYTES + 16]
            .copy_from_slice(&solid_pattern(0, 1)[0..TILE_BYTES]);
        let mut palette = [0u8; 32];
        palette[1] = 0x21; // background color 1
        palette[SPRITE_PALETTE_OFFSET + 1] = 0x16; // sprite color 1
        let base = bg(&nt, &pattern, &palette, 0, 0);
        let bg_frame = render(|f| f.background(0..SCREEN_H, &base));
        let mut oam = [0u8; 256];
        oam[0] = 0; // top scanline 1
        oam[1] = 2; // tile 2
        oam[2] = 0x20; // behind the background
        let frame = render(|f| {
            f.background(0..SCREEN_H, &base);
            f.sprites(
                0..SCREEN_H,
                &Sprites {
                    oam: &oam,
                    pattern: &pattern,
                    pattern_base: 0,
                    tall: false,
                    palette: &palette,
                },
            );
        });
        assert_eq!(pixel(&frame, 0, 1), pixel(&bg_frame, 0, 1));

        oam[2] = 0; // in front
        let frame = render(|f| {
            f.background(0..SCREEN_H, &base);
            f.sprites(
                0..SCREEN_H,
                &Sprites {
                    oam: &oam,
                    pattern: &pattern,
                    pattern_base: 0,
                    tall: false,
                    palette: &palette,
                },
            );
        });
        assert_eq!(pixel(&frame, 0, 1), palette::rgba(0x16));
    }

    #[test]
    fn tall_sprite_bit0_selects_pattern_table() {
        let nt = two_nametables();
        // Lower pattern table is empty; tile 2 of the upper table is solid.
        let mut pattern = vec![0u8; 8192];
        let off = 0x1000 + 2 * TILE_BYTES;
        pattern[off..off + 8].fill(0xff);
        let mut palette = [0u8; 32];
        palette[SPRITE_PALETTE_OFFSET + 1] = 0x28;
        let mut oam = [0u8; 256];
        oam[0] = 0;
        oam[1] = 0x03; // bit 0 set -> $1000, tile pair 2/3
        oam[3] = 0;
        let frame = render(|f| {
            f.clear();
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 0, 0));
            f.sprites(
                0..SCREEN_H,
                &Sprites {
                    oam: &oam,
                    pattern: &pattern,
                    pattern_base: 0,
                    tall: true,
                    palette: &palette,
                },
            );
        });
        assert_eq!(pixel(&frame, 0, 1), palette::rgba(0x28));
    }

    #[test]
    fn bands_can_change_scroll_and_palette() {
        // The raster/streaming engine drives the renderer band by band, so a
        // band must be able to use its own scroll and its own palette.
        let mut nt = two_nametables();
        nt[0] = 1; // tile row 0: colour 1
        nt[512] = 2; // tile row 16: colour 2, reached by an 8 px scroll
        let mut pattern = solid_pattern(1, 1);
        pattern[2 * TILE_BYTES..2 * TILE_BYTES + 16]
            .copy_from_slice(&solid_pattern(0, 2)[0..TILE_BYTES]);
        let mut palette = [0u8; 32];
        palette[0] = 0x0f; // universal background colour
        palette[1] = 0x21;
        let mut band_palette = [0u8; 32];
        band_palette[0] = 0x0f;
        band_palette[2] = 0x28;

        let frame = render(|f| {
            // Top band: palette A, no scroll (tile row 0 -> colour 1).
            f.background(0..120, &bg(&nt, &pattern, &palette, 0, 0));
            // Bottom band: palette B and a vertical scroll of 8 px, which moves
            // it to tile row 16 -> colour 2.
            f.background(120..SCREEN_H, &bg(&nt, &pattern, &band_palette, 0, 8));
        });
        assert_eq!(pixel(&frame, 0, 0), palette::rgba(0x21), "top band colour");
        assert_eq!(
            pixel(&frame, 0, 119),
            palette::rgba(0x0f),
            "top band, empty row"
        );
        assert_eq!(
            pixel(&frame, 0, 120),
            palette::rgba(0x28),
            "bottom band: its own palette and its own scroll"
        );
    }

    #[test]
    fn horizontal_scroll_changes_the_view() {
        let mut nt = two_nametables();
        nt[0] = 1; // first tile column
        nt[1] = 2; // second tile column
        let mut pattern = solid_pattern(1, 1);
        pattern[2 * TILE_BYTES..2 * TILE_BYTES + 16]
            .copy_from_slice(&solid_pattern(0, 2)[0..TILE_BYTES]);
        let mut palette = [0u8; 32];
        palette[0] = 0x0f;
        palette[1] = 0x21;
        palette[2] = 0x28;

        let unscrolled = render(|f| {
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 0, 0));
        });
        let scrolled = render(|f| {
            f.background(0..SCREEN_H, &bg(&nt, &pattern, &palette, 8, 0));
        });
        assert_eq!(pixel(&unscrolled, 0, 0), palette::rgba(0x21), "tile 1");
        assert_eq!(
            pixel(&scrolled, 0, 0),
            palette::rgba(0x28),
            "an 8 px scroll shows tile 2"
        );
    }

    #[test]
    fn clear_resets_the_frame() {
        let mut frame = Frame::new();
        frame.rgba[0] = 0xff;
        frame.opaque[0] = true;
        frame.clear();
        assert_eq!(frame.rgba[0], 0);
        assert!(!frame.opaque[0]);
    }

    /// Render into a fresh frame and hand it back.
    fn render(draw: impl FnOnce(&mut Frame)) -> Frame {
        let mut frame = Frame::new();
        draw(&mut frame);
        frame
    }

    fn bg<'a>(
        nametables: &'a [u8],
        pattern: &'a [u8],
        palette: &'a [u8; 32],
        scroll_x: u16,
        scroll_y: u16,
    ) -> Background<'a> {
        Background {
            nametables,
            pattern,
            pattern_base: 0,
            palette,
            mirroring: Mirroring::Vertical,
            scroll_x,
            scroll_y,
        }
    }
}
