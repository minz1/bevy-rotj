//! The banner's cutscene (bank 2 `$98BD`): code and data copied from CHR banks
//! `$F7-$FF` into WRAM (`$99A9`) and run from `$6000`, one picture per stage.

use super::Game;
use super::oam::{DrawCall, Metasprite};
use super::script::{Flow, Runner, wait};
use crate::levelmap::PAYLOAD_FIRST_BANK;
use crate::render::{ctrl, mask};

/// CHR offset of the payload's WRAM `$6000-$7E5F` (banks `$F8-$FF`).
const PAYLOAD: usize = 0xF8 * 0x400;
/// CHR offset of the payload's WRAM `$7E60-$7FFF` (bank `$F7` from `$260`).
const PAYLOAD_TAIL: usize = 0xF7 * 0x400 + 0x260;

/// The WRAM byte at `addr` once `$99A9` has copied the payload.
#[must_use]
pub fn payload_byte(chr: &[u8], addr: u16) -> u8 {
    let offset = usize::from(addr - 0x6000);
    if offset < 0x1E60 {
        chr[PAYLOAD + offset]
    } else {
        chr[PAYLOAD_TAIL + offset - 0x1E60]
    }
}

fn payload_word(chr: &[u8], addr: u16) -> u16 {
    u16::from_le_bytes([payload_byte(chr, addr), payload_byte(chr, addr + 1)])
}

fn payload_bytes(chr: &[u8], addr: u16, len: u16) -> Vec<u8> {
    (0..len).map(|i| payload_byte(chr, addr + i)).collect()
}

/// Reads `len` WRAM bytes at `addr` through `read`.
pub(super) fn read_bytes(read: &impl Fn(u16) -> u8, addr: u16, len: u16) -> Vec<u8> {
    (0..len).map(|i| read(addr + i)).collect()
}

/// The cutscene, one frame per [`Cutscene::step`]: the screen cleared (`$7EC9`), the
/// picture's palette, banks and nametable blocks (`$7E69`), the two halves slid in by
/// the raster IRQ (bank 2 `$9731`), a grey flash, the picture's sprite and lit palette
/// held `$80` frames, and the screen cleared again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cutscene {
    /// Where the cutscene is in [`SCRIPT`].
    runner: Runner,
    /// The stage's picture (`$2B`, then `$76D1[$2B]`).
    scene: u8,
    clear: ScreenClear,
    /// The draw list's read offset (`$20`).
    cursor: u8,
}

/// What the payload does from `$6000` on, in order (the rows of [`SCRIPT`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// `$99A9`'s three frames.
    CopyPayload,
    /// The screen cleared (`$7EC9`).
    Clear,
    /// The picture's song, banks and dark palette, for a frame.
    Picture,
    /// The picture's nametable blocks, one a frame (`$7E69`, `$775C[scene]` of them).
    PictureBlocks,
    /// The raster slide set up, for a frame (bank 2 `$9731`).
    Slide,
    /// Until the slide's counters (`$D5`, `$D6`, `$DA`) run out.
    SlideIn,
    /// The split off, the PPU back on the first nametable.
    SlideEnd,
    /// `$612D`'s count of frames.
    Wait(u16),
    /// The grey flash (`$3F00`, all `$10`).
    GreyPalette,
    /// The picture's sprite and lit palette.
    LightPicture,
    /// The music stopped (`$40 = $FF`).
    StopMusic,
}

/// The cutscene: copy, clear, the picture slid in, a grey flash, the lit picture, and the
/// screen cleared again.
const SCRIPT: &[Row] = &[
    Row::CopyPayload,
    Row::Clear,
    Row::Picture,
    Row::PictureBlocks,
    Row::Slide,
    Row::SlideIn,
    Row::SlideEnd,
    Row::Wait(0x20),
    Row::GreyPalette,
    Row::Wait(0x21),
    Row::LightPicture,
    Row::Wait(0x81),
    Row::StopMusic,
    Row::Clear,
];

impl Cutscene {
    /// The cutscene for picture `scene` (`$93C5[$0400 / 2]`).
    #[must_use]
    pub fn new(scene: u8) -> Cutscene {
        Cutscene {
            runner: Runner::default(),
            scene,
            clear: ScreenClear::new(0),
            cursor: 0,
        }
    }

    /// Runs one frame up to its NMI wait; true once the cutscene has returned
    /// (`$612C`), with the rest of this frame left to the caller.
    pub fn step(&mut self, game: &mut Game, prg: &[u8], chr: &[u8]) -> bool {
        loop {
            let Some(&row) = self.runner.row(SCRIPT) else {
                return true;
            };
            let flow = self.run(row, game, prg, chr);
            if self.runner.advance(flow) {
                return false;
            }
        }
    }

    /// Runs a row's frame, or the whole of a row that takes none.
    fn run(&mut self, row: Row, game: &mut Game, prg: &[u8], chr: &[u8]) -> Flow {
        let frame = self.runner.frame;
        match row {
            Row::CopyPayload => copy_payload(game, prg, frame, PAYLOAD_FIRST_BANK),
            Row::Clear => {
                if frame == 0 {
                    self.clear = ScreenClear::new(0);
                }
                if self.clear.step(game) {
                    Flow::Done
                } else {
                    Flow::Waited
                }
            }
            Row::Picture => {
                if frame == 0 {
                    self.picture(game, chr);
                }
                wait(frame, 1)
            }
            Row::PictureBlocks => {
                let count = payload_byte(chr, 0x775C + 2 * u16::from(self.scene));
                if frame == u16::from(count) {
                    return Flow::Done;
                }
                draw_block(
                    game,
                    |addr| payload_byte(chr, addr),
                    0x71DD,
                    &mut self.cursor,
                );
                Flow::Waited
            }
            Row::Slide => {
                if frame == 0 {
                    let at = 0x7755 + 2 * u16::from(self.scene);
                    let split = &mut game.scroll.split;
                    split.handler = 0x0A;
                    split.next_irq = 0;
                    split.first_irq_cycles =
                        u16::from_le_bytes([payload_byte(chr, at), payload_byte(chr, at + 1)]);
                    split.armed = true;
                    game.scroll.mask = mask::SHOWN;
                }
                wait(frame, 1)
            }
            Row::SlideIn => {
                let bands = &game.scroll.bands;
                if bands[0] | bands[1] | bands[5] != 0 {
                    Flow::Waited
                } else {
                    Flow::Done
                }
            }
            Row::SlideEnd => {
                game.scroll.split.armed = false;
                game.scroll.ppu_ctrl = ctrl::NMI | ctrl::TALL_SPRITES | ctrl::SPRITE_PATTERNS_HIGH;
                Flow::Done
            }
            Row::Wait(count) => wait(frame, count),
            Row::GreyPalette => {
                game.effects.vram.push_palette(&[0x10; 32]);
                Flow::Done
            }
            Row::LightPicture => {
                self.light(game, chr);
                Flow::Done
            }
            Row::StopMusic => {
                game.effects.music = 0xFF;
                Flow::Done
            }
        }
    }

    /// The picture's song, CHR banks and dark palette (`$600B-$6053`).
    fn picture(&mut self, game: &mut Game, chr: &[u8]) {
        game.effects.song = 0x0F;
        self.scene = payload_byte(chr, 0x76D1 + u16::from(self.scene));
        let banks = 0x76D7 + u16::from(self.scene) * 8;
        for (i, shadow) in (0..).zip(&mut game.oam.chr) {
            *shadow = payload_byte(chr, banks + i);
        }
        game.scroll.ppu_ctrl = ctrl::NMI | ctrl::TALL_SPRITES | ctrl::SPRITE_PATTERNS_HIGH;
        game.scroll.scroll_y = 0;
        game.scroll.scroll_x = 0;
        game.scroll.bands[0] = 0;
        game.scroll.bands[1] = 0;
        game.scroll.bands[5] = 3;
        game.oam.clear();
        let palette = payload_bytes(chr, 0x76EF, 32);
        game.effects.vram.push_palette(&palette);
        self.cursor = payload_byte(chr, 0x775B + 2 * u16::from(self.scene));
    }

    /// The picture's sprite (`$C247`, `$C145`) and its lit palette (`$60C4-$6113`).
    fn light(&self, game: &mut Game, chr: &[u8]) {
        let at = 0x7653 + 4 * u16::from(self.scene);
        let pointer = payload_word(chr, at);
        let (x, y) = (payload_byte(chr, at + 2), payload_byte(chr, at + 3));
        draw_sprite(game, |addr| payload_byte(chr, addr), pointer, x, y);
        game.oam.finish();
        let lit = u16::from_le_bytes([
            payload_byte(chr, 0x771F + 2 * u16::from(self.scene)),
            payload_byte(chr, 0x7720),
        ]);
        let mut palette = payload_bytes(chr, 0x770F, 16);
        palette.extend(payload_bytes(chr, lit, 16));
        game.effects.vram.push_palette(&palette);
    }
}

/// A script row copying the payload from the eight CHR banks from `first` (`$99A9`, the
/// boot's `$98C3`): three frames of [`copy_frame`].
pub(super) fn copy_payload(game: &mut Game, prg: &[u8], frame: u16, first: u8) -> Flow {
    if frame < 3 {
        let frame = u8::try_from(frame).expect("< 3");
        copy_frame(game, prg, frame, first);
    }
    wait(frame, 3)
}

/// One of a payload copy's three frames (`$99A9`, the boot's `$98C3`), each ending at an
/// NMI wait: the CHR shadows set to the eight banks from `first` and WRAM enabled with
/// rendering off (`$9985`) for the copy, NMIs back on (`$996D`), and the dark palette
/// (`$C35A`).
pub(super) fn copy_frame(game: &mut Game, prg: &[u8], frame: u8, first: u8) {
    match frame {
        0 => {
            for (shadow, bank) in game.oam.chr.iter_mut().zip(first..=first + 7) {
                *shadow = bank;
            }
            game.display.prg_windows[0] = 0xC0;
            game.scroll.split.armed = false;
            game.scroll.ppu_ctrl &= !(ctrl::NMI | ctrl::VERTICAL_INCREMENT);
            game.scroll.mask = 0;
        }
        1 => game.scroll.ppu_ctrl |= ctrl::NMI,
        _ => super::stage::blackout(game, prg),
    }
}

/// The payload's screen clear (`$7EC9`, `$2F = 0`), one frame per [`ScreenClear::step`]:
/// a black palette, then both nametables filled with `$2E` 64 bytes a frame and their
/// attributes zeroed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScreenClear {
    /// The frame (`$2C`).
    frame: u8,
    /// The nametables' tile (`$2E`).
    fill: u8,
}

impl ScreenClear {
    pub(super) fn new(fill: u8) -> ScreenClear {
        ScreenClear { frame: 0, fill }
    }

    /// Frames run so far.
    pub(super) fn frame(self) -> u8 {
        self.frame
    }

    /// One frame of the clear up to its NMI wait; true, without a frame, once it has run.
    pub(super) fn step(&mut self, game: &mut Game) -> bool {
        let frame = self.frame;
        self.frame += 1;
        match frame {
            0 => game.scroll.split.armed = false,
            1 => {
                game.scroll.mirroring = 0;
                game.scroll.scroll_x = 0;
                game.scroll.scroll_y = 0;
                game.scroll.mask = 0;
                game.scroll.ppu_ctrl = ctrl::NMI | ctrl::TALL_SPRITES | ctrl::SPRITE_PATTERNS_HIGH;
                game.oam.clear();
                game.effects.vram.push_palette(&[0x0F; 32]);
            }
            2..=33 => {
                let addr = 0x2000 + u16::from(frame - 2) * 0x40;
                game.effects.vram.push(addr, 0, &[self.fill; 0x40]);
            }
            34 | 35 => {
                let addr = 0x23C0 + u16::from(frame - 34) * 0x400;
                game.effects.vram.push(addr, 0, &[0; 0x40]);
            }
            _ => return true,
        }
        false
    }
}

/// One block of a draw list's nametable runs (`$7E69`, the list at `list`, read from
/// `cursor` on): each entry queues `count` runs of `len` bytes, down a row or across a
/// byte apart. `read` is WRAM.
pub(super) fn draw_block(game: &mut Game, read: impl Fn(u16) -> u8, list: u16, cursor: &mut u8) {
    let byte = |at: u8| read(list + u16::from(at));
    let entries = byte(*cursor);
    *cursor = cursor.wrapping_add(1);
    for _ in 0..entries {
        let entry = [0u8, 1, 2, 3, 4, 5, 6].map(|i| byte(cursor.wrapping_add(i)));
        let [count, len, flags, ..] = entry;
        let mut source = u16::from_le_bytes([entry[3], entry[4]]);
        let [mut low, mut high] = [entry[5], entry[6]];
        *cursor = cursor.wrapping_add(7);
        for _ in 0..count {
            let addr = u16::from_le_bytes([low, high]);
            let run = read_bytes(&read, source, u16::from(len));
            game.effects.vram.push(addr, flags, &run);
            if flags == 0 {
                let [next_low, next_high] = addr.wrapping_add(0x20).to_le_bytes();
                (low, high) = (next_low, next_high);
            } else {
                low = low.wrapping_add(1);
            }
            source = source.wrapping_add(u16::from(len));
        }
    }
}

/// Draws the WRAM metasprite at `pointer` at screen (`x`, `y`) with no attribute, tile
/// or CHR offset (`$94-$9A`, `$A4` set, then `$C247`).
pub(super) fn draw_sprite(game: &mut Game, read: impl Fn(u16) -> u8, pointer: u16, x: u8, y: u8) {
    draw_sprite_with(game, read, pointer, (x, y), 0);
}

/// [`draw_sprite`] with the attribute `attr` (`$99`).
pub(super) fn draw_sprite_with(
    game: &mut Game,
    read: impl Fn(u16) -> u8,
    pointer: u16,
    (x, y): (u8, u8),
    attr: u8,
) {
    let work = &mut game.work;
    work.pointer = pointer;
    work.screen_x = x;
    work.screen_y = y;
    work.chr_slot = 0;
    work.attr = attr;
    work.anchor = 0;
    work.tile = 0;
    let len = read(pointer);
    let bytes = read_bytes(&read, pointer, u16::from(len) + 1);
    let sprite = Metasprite::from_bytes(&bytes).expect("a payload metasprite");
    let call = DrawCall {
        x: i16::from(x),
        y: i16::from(y),
        attr,
        tile: 0,
        chr_slot: 0,
        bottom: false,
    };
    game.oam.draw(&sprite, call, game.world.frame_odd);
}
