//! 2A03 synthesis cores for the Batman: Return of the Joker port.
//!
//! The ROTJ cart's FME-7 board has no expansion audio, so the game runs the
//! classic 2A03 channels: two pulses, triangle, noise, and DMC (the game's
//! famous sampled bass lines come through DMC).
//!
//! Cores provided as channel synthesizers with register-write-style APIs
//! mirroring the hardware map, all documented on the channels:
//!
//! - [`pulse::Pulse`] with pulse channel selection ([`pulse::PulseChannel`])
//! - [`triangle::Triangle`]
//! - [`noise::Noise`] with LFSR feedback selection ([`noise::ShiftMode`])
//! - [`frame_counter::FrameCounter`], driving the channels with
//!   [`frame_counter::FrameEvent`] via the channels'
//!   `clock_quarter_frame`/`clock_half_frame`
//! - [`apu::Apu`]: the register file (`$4000-$4017`) that ties the channels and
//!   the frame sequencer together, with [`mixer`] producing the analog level
//! - [`dmc::Dmc`] with the sample-bank reader ([`dmc::SampleRom`]); samples are
//!   read from the ROM's fixed `$E000` bank (PRG bank 15)
//!
//! Semantics are written against the nesdev APU pages, with pivot points
//! (duty-table phase direction and timer divider math, envelope divider and
//! restart order, length-halt re-buffering, frame-counter step tables
//! 7457/14913/22371/29828/29829, sequence-table step order) cross-checked
//! against Tetanes, `<https://github.com/lukexor/tetanes>` (MIT).
//!
//! Status: all five channels are implemented; the DMC sample bank is supplied
//! by the caller ([`apu::Apu::set_sample_rom`]) from the ROM's PRG bank 15. No
//! output device lives here (that is the `app` integration's job, via
//! [`stream`]), and the frame counter and DMC expose `irq_pending()` for
//! polling rather than CPU-side IRQ plumbing.

pub mod apu;
pub mod dmc;
pub mod frame_counter;
pub mod mixer;
pub mod noise;
pub mod pulse;
pub mod stream;
pub mod triangle;

mod envelope;
mod length_counter;
mod timer;
