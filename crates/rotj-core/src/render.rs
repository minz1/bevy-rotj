//! Software renderer that reproduces the NES PPU's picture.
//!
//! [`Frame::render`] draws a 256x240 RGBA image line by line from the inputs the PPU
//! has (nametables, CHR, palette RAM, OAM) and each line's own scroll, `$2000`/`$2001`
//! and CHR banks, so mid-frame changes show up exactly where the hardware shows them.

use crate::chr::TILE_BYTES;
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
/// Sprites in OAM, so a per-line limit of this many is no limit.
pub const OAM_SPRITES: usize = 64;

/// A reusable frame buffer: RGBA output and the palette-RAM value behind every
/// pixel (for exact verification against emulator output).
#[derive(Debug, Clone)]
pub struct Frame {
    rgba: Vec<u8>,
    indices: Vec<u8>,
}

impl Frame {
    /// A cleared frame.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rgba: vec![0; FRAME_BYTES],
            indices: vec![0; SCREEN_W * SCREEN_H],
        }
    }

    /// Reset to fully transparent.
    pub fn clear(&mut self) {
        self.rgba.fill(0);
        self.indices.fill(0);
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
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

/// What the PPU draws one scanline with: the scroll, `$2000`/`$2001`, and the mapper's
/// CHR banks and mirroring as they were when the line began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// The VRAM address of the line's first tile: coarse x (bits 0-4) and y (5-9), the
    /// nametable (10-11), fine y (12-14).
    pub v: u16,
    /// Fine X scroll.
    pub fine_x: u8,
    /// `$2000`: pattern tables (bits 3, 4), sprite size (bit 5).
    pub ctrl: u8,
    /// `$2001`: greyscale (bit 0), left-8 background/sprites (bits 1, 2), background and
    /// sprites on (bits 3, 4).
    pub mask: u8,
    /// The eight 1 KiB CHR banks of `$0000-$1FFF`, for the background.
    pub chr: [u8; 8],
    /// The banks the line's sprites were fetched with (in the previous line's hblank).
    pub sprite_chr: [u8; 8],
    /// 0 vertical, 1 horizontal, 2 single-screen low, 3 single-screen high.
    pub mirroring: u8,
}

/// Where a line's background pixel `x` is fetched from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundCell {
    /// The physical nametable, 0 or 1, as the line's mirroring maps it.
    pub physical: usize,
    /// The tile byte's offset in it.
    pub tile_at: usize,
    /// The attribute byte's offset in it.
    pub attribute_at: usize,
    /// The palette bits' shift in the attribute byte.
    pub shift: usize,
    /// The pixel's column within its tile, 0-7.
    pub column: usize,
}

impl Line {
    /// Drawn scrolled sideways: the column the game is still writing shows at its edges,
    /// which a TV's overscan hid.
    #[must_use]
    pub const fn scrolled(&self) -> bool {
        self.v & 0x1F != 0 || self.fine_x != 0
    }

    /// Where the line's background pixel `x` (0-255) is fetched from.
    #[must_use]
    pub fn background_cell(&self, x: usize) -> BackgroundCell {
        let v = usize::from(self.v);
        let coarse_y = (v >> 5) & 0x1F;
        let column = (v & 0x1F) * 8 + usize::from(self.fine_x) + x;
        let coarse_x = (column / 8) % 32;
        let table = ((v >> 10) & 3) ^ ((column / 256) & 1);
        let physical = match self.mirroring & 3 {
            0 => table & 1,
            1 => (table >> 1) & 1,
            2 => 0,
            _ => 1,
        };
        BackgroundCell {
            physical,
            tile_at: coarse_y * 32 + coarse_x,
            attribute_at: ATTRIBUTE_OFFSET + (coarse_y / 4) * 8 + coarse_x / 4,
            shift: ((coarse_y % 4) / 2) * 4 + ((coarse_x % 4) / 2) * 2,
            column: column % 8,
        }
    }
}

/// A whole frame's inputs as the PPU sees them.
#[derive(Clone, Copy, Debug)]
pub struct Picture<'a> {
    /// The 240 visible lines.
    pub lines: &'a [Line],
    /// The console's 2 KiB of nametable RAM.
    pub vram: &'a [u8],
    /// Palette RAM.
    pub palette: &'a [u8; 32],
    /// Object attribute memory.
    pub oam: &'a [u8; 256],
    /// The whole CHR ROM.
    pub chr: &'a [u8],
}

impl Picture<'_> {
    fn pattern(&self, banks: [u8; 8], addr: usize) -> u8 {
        let bank = usize::from(banks[(addr >> 10) & 7]) * 1024 % self.chr.len();
        self.chr[bank + (addr & 0x3FF)]
    }

    fn nametable(&self, physical: usize, at: usize) -> u8 {
        self.vram[physical * NAMETABLE_BYTES + at]
    }

    fn colour(&self, line: &Line, entry: usize) -> u8 {
        let value = self.palette[if entry.trailing_zeros() >= 2 {
            0
        } else {
            entry
        }] & 0x3F;
        if line.mask & 1 != 0 {
            value & 0x30
        } else {
            value
        }
    }
}

impl Frame {
    /// Renders a frame line by line from what the PPU had on each line: the background
    /// from the line's scroll and banks, then up to `sprites_per_line` sprites per line
    /// (the PPU's [`MAX_SPRITES_PER_LINE`]), the lowest OAM index in front, behind the
    /// background where their priority bit says, clipped at the right edge and by
    /// `$2001`'s left-8 bits.
    pub fn render(&mut self, picture: &Picture<'_>, sprites_per_line: usize) {
        self.clear();
        let mut bg = [0u8; SCREEN_W];
        for (y, line) in picture.lines.iter().enumerate().take(SCREEN_H) {
            self.background_line(picture, line, y, &mut bg);
            self.sprite_line(picture, line, y, &bg, sprites_per_line);
        }
    }

    fn background_line(
        &mut self,
        picture: &Picture<'_>,
        line: &Line,
        y: usize,
        bg: &mut [u8; SCREEN_W],
    ) {
        let fine_y = (usize::from(line.v) >> 12) & 7;
        let base = if line.ctrl & 0x10 != 0 { 0x1000 } else { 0 };
        for (x, slot) in bg.iter_mut().enumerate() {
            let cell = line.background_cell(x);
            let tile = picture.nametable(cell.physical, cell.tile_at);
            let attr = picture.nametable(cell.physical, cell.attribute_at);
            let shift = cell.shift;
            let addr = base + usize::from(tile) * TILE_BYTES + fine_y;
            let bit = 7 - cell.column;
            let low = (picture.pattern(line.chr, addr) >> bit) & 1;
            let high = (picture.pattern(line.chr, addr + 8) >> bit) & 1;
            let shown = line.mask & 0x08 != 0 && (x >= 8 || line.mask & 0x02 != 0);
            let value = if shown { high << 1 | low } else { 0 };
            *slot = value;
            let entry = usize::from((attr >> shift) & 3) * 4 + usize::from(value);
            self.put(y * SCREEN_W + x, picture.colour(line, entry));
        }
    }

    fn sprite_line(
        &mut self,
        picture: &Picture<'_>,
        line: &Line,
        y: usize,
        bg: &[u8; SCREEN_W],
        sprites_per_line: usize,
    ) {
        if line.mask & 0x10 == 0 {
            return;
        }
        let tall = line.ctrl & 0x20 != 0;
        let height = if tall { 16 } else { 8 };
        let mut drawn = [false; SCREEN_W];
        let found = (0..OAM_SPRITES)
            .map(|i| &picture.oam[i * 4..i * 4 + 4])
            .filter(|oam| (usize::from(oam[0]) + 1..usize::from(oam[0]) + 1 + height).contains(&y))
            .take(sprites_per_line);
        for oam in found {
            let (attr, x0) = (oam[2], usize::from(oam[3]));
            let row = y - usize::from(oam[0]) - 1;
            let row = if attr & 0x80 != 0 {
                height - 1 - row
            } else {
                row
            };
            let addr = if tall {
                let table = if oam[1] & 1 != 0 { 0x1000 } else { 0 };
                table + usize::from(oam[1] & 0xFE) * TILE_BYTES + (row / 8) * TILE_BYTES + row % 8
            } else {
                let table = if line.ctrl & 0x08 != 0 { 0x1000 } else { 0 };
                table + usize::from(oam[1]) * TILE_BYTES + row
            };
            let (low, high) = (
                picture.pattern(line.sprite_chr, addr),
                picture.pattern(line.sprite_chr, addr + 8),
            );
            for col in 0..8 {
                let x = x0 + col;
                if x >= SCREEN_W || drawn[x] || (x < 8 && line.mask & 0x04 == 0) {
                    continue;
                }
                let bit = if attr & 0x40 != 0 { col } else { 7 - col };
                let value = ((high >> bit) & 1) << 1 | ((low >> bit) & 1);
                if value == 0 {
                    continue;
                }
                drawn[x] = true;
                if attr & 0x20 == 0 || bg[x] == 0 {
                    let entry =
                        SPRITE_PALETTE_OFFSET + usize::from(attr & 3) * 4 + usize::from(value);
                    self.put(y * SCREEN_W + x, picture.colour(line, entry));
                }
            }
        }
    }

    fn put(&mut self, at: usize, value: u8) {
        self.indices[at] = value;
        self.rgba[at * 4..at * 4 + 4].copy_from_slice(&palette::rgba(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANKS: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

    struct Scene {
        vram: Vec<u8>,
        palette: [u8; 32],
        oam: [u8; 256],
        chr: Vec<u8>,
        line: Line,
    }

    impl Scene {
        fn new() -> Self {
            Self {
                vram: vec![0; NAMETABLE_BYTES * 2],
                palette: [0; 32],
                oam: [0xFF; 256],
                chr: vec![0; 8192],
                line: Line {
                    mask: 0x1E,
                    chr: BANKS,
                    sprite_chr: BANKS,
                    ..Line::default()
                },
            }
        }

        fn solid(&mut self, table: usize, tile: usize, index: u8) {
            let at = table * 0x1000 + tile * TILE_BYTES;
            self.chr[at..at + 8].fill(if index & 1 != 0 { 0xFF } else { 0 });
            self.chr[at + 8..at + 16].fill(if index & 2 != 0 { 0xFF } else { 0 });
        }

        fn sprite(&mut self, n: usize, y: u8, tile: u8, attr: u8, x: u8) {
            self.oam[n * 4..n * 4 + 4].copy_from_slice(&[y, tile, attr, x]);
        }

        fn render(&self) -> Frame {
            self.render_with(MAX_SPRITES_PER_LINE)
        }

        fn render_with(&self, sprites_per_line: usize) -> Frame {
            let lines = [self.line; SCREEN_H];
            let mut frame = Frame::new();
            frame.render(
                &Picture {
                    lines: &lines,
                    vram: &self.vram,
                    palette: &self.palette,
                    oam: &self.oam,
                    chr: &self.chr,
                },
                sprites_per_line,
            );
            frame
        }
    }

    fn at(frame: &Frame, x: usize, y: usize) -> u8 {
        frame.indices()[y * SCREEN_W + x]
    }

    #[test]
    fn background_attribute_selects_palette() {
        let mut scene = Scene::new();
        scene.vram[0] = 1;
        scene.vram[ATTRIBUTE_OFFSET] = 0b10;
        scene.solid(0, 1, 1);
        scene.palette[0] = 0x0F;
        scene.palette[2 * 4 + 1] = 0x21;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 0), 0x21);
        assert_eq!(frame.rgba()[..4], palette::rgba(0x21));
    }

    #[test]
    fn background_index_zero_uses_universal_color() {
        let mut scene = Scene::new();
        scene.solid(0, 1, 1);
        scene.palette[0] = 0x0F;
        scene.palette[1] = 0x21;
        assert_eq!(at(&scene.render(), 0, 0), 0x0F);
    }

    #[test]
    fn background_scroll_reaches_second_nametable() {
        let mut scene = Scene::new();
        scene.vram[31] = 1;
        scene.vram[NAMETABLE_BYTES] = 2;
        scene.solid(0, 1, 1);
        scene.solid(0, 2, 2);
        scene.palette[1] = 0x21;
        scene.palette[2] = 0x11;
        scene.line.v = 31;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 0), 0x21);
        assert_eq!(at(&frame, 8, 0), 0x11);
    }

    #[test]
    fn sprite_priority_bit_keeps_background_in_front() {
        let mut scene = Scene::new();
        scene.vram[0] = 1;
        scene.solid(0, 1, 1);
        scene.solid(0, 2, 1);
        scene.palette[1] = 0x21;
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.sprite(0, 0, 2, 0x20, 0);
        assert_eq!(at(&scene.render(), 0, 1), 0x21);
        scene.sprite(0, 0, 2, 0, 0);
        assert_eq!(at(&scene.render(), 0, 1), 0x16);
    }

    #[test]
    fn tall_sprite_bit0_selects_pattern_table() {
        let mut scene = Scene::new();
        scene.solid(1, 2, 1);
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x28;
        scene.line.ctrl = 0x20;
        scene.sprite(0, 0, 3, 0, 0);
        assert_eq!(at(&scene.render(), 0, 1), 0x28);
    }

    #[test]
    fn horizontal_scroll_changes_the_view() {
        let mut scene = Scene::new();
        scene.vram[0] = 1;
        scene.vram[1] = 2;
        scene.solid(0, 1, 1);
        scene.solid(0, 2, 2);
        scene.palette[1] = 0x21;
        scene.palette[2] = 0x28;
        assert_eq!(at(&scene.render(), 0, 0), 0x21);
        scene.line.v = 1;
        assert_eq!(at(&scene.render(), 0, 0), 0x28);
        scene.line.v = 0;
        scene.line.fine_x = 4;
        let frame = scene.render();
        assert_eq!(at(&frame, 3, 0), 0x21);
        assert_eq!(at(&frame, 4, 0), 0x28);
    }

    #[test]
    fn clear_resets_the_frame() {
        let mut frame = Frame::new();
        frame.rgba[0] = 0xFF;
        frame.indices[0] = 0x21;
        frame.clear();
        assert_eq!(frame.rgba[0], 0);
        assert_eq!(frame.indices[0], 0);
    }

    #[test]
    fn the_lowest_oam_index_is_in_front() {
        let mut scene = Scene::new();
        scene.solid(0, 1, 1);
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.palette[SPRITE_PALETTE_OFFSET + 5] = 0x28;
        scene.sprite(0, 0, 1, 0, 0);
        scene.sprite(1, 0, 1, 1, 0);
        assert_eq!(at(&scene.render(), 0, 1), 0x16);
    }

    #[test]
    fn sprites_clip_at_the_right_edge() {
        let mut scene = Scene::new();
        scene.solid(0, 1, 1);
        scene.palette[0] = 0x0F;
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.sprite(0, 0, 1, 0, 252);
        let frame = scene.render();
        for x in 252..256 {
            assert_eq!(at(&frame, x, 1), 0x16, "x {x}");
        }
        for x in 0..4 {
            assert_eq!(at(&frame, x, 1), 0x0F, "x {x}");
        }
    }

    #[test]
    fn sprite_size_and_table_follow_ctrl() {
        let mut scene = Scene::new();
        scene.solid(0, 4, 1);
        scene.solid(0, 5, 1);
        scene.solid(1, 5, 2);
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.palette[SPRITE_PALETTE_OFFSET + 2] = 0x28;
        scene.sprite(0, 0, 5, 0, 0);
        assert_eq!(at(&scene.render(), 0, 1), 0x16);
        scene.line.ctrl = 0x08;
        assert_eq!(at(&scene.render(), 0, 1), 0x28);

        scene.solid(0, 5, 2);
        scene.sprite(0, 0, 4, 0, 0);
        scene.line.ctrl = 0x00;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 8), 0x16);
        assert_eq!(at(&frame, 0, 9), 0);
        scene.line.ctrl = 0x20;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 8), 0x16);
        assert_eq!(at(&frame, 0, 9), 0x28);
    }

    #[test]
    fn left_eight_pixels_clip() {
        let mut scene = Scene::new();
        scene.vram[..960].fill(1);
        scene.solid(0, 1, 1);
        scene.solid(0, 2, 1);
        scene.palette[0] = 0x0F;
        scene.palette[1] = 0x21;
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.sprite(0, 0, 2, 0, 0);
        scene.sprite(1, 0, 2, 0, 8);
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 1), 0x16);
        assert_eq!(at(&frame, 7, 1), 0x16);
        scene.line.mask = 0x18;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 1), 0x0F);
        assert_eq!(at(&frame, 7, 1), 0x0F);
        assert_eq!(at(&frame, 8, 1), 0x16);
        assert_eq!(at(&frame, 16, 1), 0x21);
    }

    #[test]
    fn greyscale_masks_the_hue() {
        let mut scene = Scene::new();
        scene.vram[0] = 1;
        scene.solid(0, 1, 1);
        scene.solid(0, 2, 1);
        scene.palette[0] = 0x0F;
        scene.palette[1] = 0x21;
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        scene.sprite(0, 0, 2, 0, 16);
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 0), 0x21);
        assert_eq!(at(&frame, 16, 1), 0x16);
        scene.line.mask = 0x1F;
        let frame = scene.render();
        assert_eq!(at(&frame, 0, 0), 0x20);
        assert_eq!(at(&frame, 16, 1), 0x10);
        assert_eq!(at(&frame, 100, 100), 0x00);
    }

    #[test]
    fn nine_sprites_on_a_line_draw_eight_with_the_limit_and_nine_without() {
        let mut scene = Scene::new();
        scene.solid(0, 1, 1);
        scene.palette[0] = 0x0F;
        scene.palette[SPRITE_PALETTE_OFFSET + 1] = 0x16;
        for n in 0..9u8 {
            scene.sprite(usize::from(n), 0, 1, 0, n * 8);
        }
        let frame = scene.render();
        assert_eq!(at(&frame, 56, 1), 0x16);
        assert_eq!(at(&frame, 64, 1), 0x0F);
        assert_eq!(at(&frame, 64, 9), 0x0F);
        let frame = scene.render_with(64);
        assert_eq!(at(&frame, 64, 1), 0x16);
        assert_eq!(at(&frame, 64, 8), 0x16);
    }
}
