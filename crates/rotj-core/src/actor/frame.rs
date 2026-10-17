//! The game loop's own pieces (bank 14 `$D3F9`): the HUD, the background CHR
//! animation and the palette upload.

use super::Game;
use super::engine::{draw, rom_word};
use super::oam::Mark;
use crate::actor::world::Motion;
use crate::prg::Prg;

/// The background CHR animation (its switch `$0401` and the palette shadow are in
/// [`Effects`](super::world::Effects)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Display {
    /// Each animated shadow's banks, eight a shadow (`$0410-$042F`).
    pub banks: [u8; 32],
    /// Each animated shadow's period, frame count, timer and frame (`$0430-$043F`).
    pub timers: [[u8; 4]; 4],
    /// The mapper shadows the NMI writes for the `$6000` window, WRAM once enabled
    /// (`$E8`), and the `$C000` window (`$EB`); the `$8000` and `$A000` windows' (`$E9`,
    /// `$EA`) change with every far call.
    pub prg_windows: [u8; 2],
}

/// Draws the energy and meter gauges (`$DB9D`).
pub fn draw_hud(game: &mut Game, prg: &Prg) {
    if game.effects.hud & 0x80 != 0 || game.effects.player.motion == Motion::Dying {
        return;
    }
    let (mut cx, work) = game.parts(prg);
    let gauge = |value: u8| rom_word(prg, 14, 0xDC0F + u16::from(value));
    work.anchor = 0;
    work.tile = 0;
    work.attr = 3;
    work.screen_x = 0x18;
    work.screen_y = 0x20;
    if cx.world.frame_odd {
        work.pointer = rom_word(prg, 14, 0xDC1F);
        cx.oam.marking = Mark::Label;
        draw(work, 14, &mut cx);
        cx.oam.marking = Mark::Hud;
    }
    let energy = cx.effects.player.energy;
    if energy & 0x80 == 0 && (energy != 0 || cx.world.frame & 8 == 0) {
        work.pointer = gauge(energy);
        draw(work, 14, &mut cx);
    }
    work.attr = 0x83;
    work.screen_y = 0x30;
    let meter = cx.effects.meter;
    if meter & 0x80 != 0 {
        return;
    }
    if meter == 0x0E && cx.world.frame & 4 == 0 {
        work.attr = 0x80;
    }
    work.pointer = gauge(meter);
    draw(work, 14, &mut cx);
}

/// Steps the animated CHR shadows (`$D2B0`).
pub fn animate_chr(game: &mut Game) {
    let display = &mut game.display;
    for (shadow, timer) in display.timers.iter_mut().enumerate() {
        if game.effects.animated & (1 << shadow) == 0 {
            continue;
        }
        let [period, frames, tick, frame] = timer;
        *tick = tick.wrapping_add(1);
        if *tick < *period {
            continue;
        }
        *tick = 0;
        *frame = frame.wrapping_add(1);
        if *frame >= *frames {
            *frame = 0;
        }
        game.oam.chr[shadow] = display.banks[shadow * 8 + usize::from(*frame)];
    }
}

/// Queues the palette when the queue is empty (`$D6D3`).
pub fn queue_palette(game: &mut Game) {
    if game.effects.vram.top < 0x7F {
        return;
    }
    let effects = &mut game.effects;
    effects.vram.push(0x3F00, 0, &effects.palette);
    effects.palette_dirty = 0;
}

#[cfg(test)]
mod layout {
    use super::Display;

    impl Display {
        pub(crate) fn lift(ram: &[u8; 2048]) -> Display {
            Display {
                banks: ram[0x410..0x430].try_into().expect("32 bytes"),
                timers: std::array::from_fn(|i| {
                    ram[0x430 + 4 * i..0x434 + 4 * i]
                        .try_into()
                        .expect("4 bytes")
                }),
                prg_windows: [ram[0xE8], ram[0xEB]],
            }
        }

        pub(crate) fn project(&self, ram: &mut [u8; 2048]) {
            ram[0x410..0x430].copy_from_slice(&self.banks);
            for (i, timer) in self.timers.iter().enumerate() {
                ram[0x430 + 4 * i..0x434 + 4 * i].copy_from_slice(timer);
            }
            [ram[0xE8], ram[0xEB]] = self.prg_windows;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::oracle::{Scene, assert_routine};
    use super::{animate_chr, draw_hud, queue_palette};

    fn check(entry: u16, exit: u16, port: impl FnMut(&mut super::super::oracle::Routine<'_>)) {
        let Some(mut scene) = Scene::tas(351) else {
            return;
        };
        let checked = assert_routine(&mut scene, 1680, (entry, 14), &[exit], port);
        assert!(checked > 1000, "${entry:04X}: {checked}");
    }

    #[test]
    fn hud_matches_db9d() {
        check(0xD3F9, 0xD3FC, |call| draw_hud(&mut call.state, call.prg));
    }

    #[test]
    fn chr_animation_matches_d2b0() {
        check(0xD456, 0xD459, |call| animate_chr(&mut call.state));
    }

    #[test]
    fn palette_matches_d6d3() {
        check(0xD459, 0xD45C, |call| queue_palette(&mut call.state));
    }
}
