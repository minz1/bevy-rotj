//! The ported sound driver.
//!
//! This is a reimplementation of the 6502 driver in PRG bank 10
//! (`disasm/bank0A.s`), not an emulator: the per-channel state arrays, the
//! song-stream opcodes, the instrument/envelope macros, the DMC record
//! playback and the SFX (`$06E5`) engine are transcribed routine-for-routine.
//! Each piece of state is named after what it does; its doc comment keeps the
//! RAM address from the disassembly.
//!
//! The driver reads its *data* tables from PRG bank 10 and its songs from PRG
//! banks 6/7 of the user's ROM ([`Music::from_prg`]).

/// Music channels the driver ticks (`$0605`.., X = 7..0).
const CHANNELS: usize = 8;
/// SFX / DMC channels (`$06E5`.., X = 1..0).
const SFX_CHANNELS: usize = 2;
/// CPU base of the extracted driver bank.
const DRIVER_BASE: u16 = 0x8000;
/// CPU base of the song-bank window.
const SONG_BASE: u16 = 0xA000;
/// First PRG bank of the song window (banks 6-7).
const SONG_BANK_BASE: u8 = 6;

/// `$885C`: note periods, low bytes.
const PERIOD_LO_TABLE: u16 = 0x885C;
/// `$87FB`: note periods, high bytes.
const PERIOD_HI_TABLE: u16 = 0x87FB;
/// `$8B91`: per song id, the header index (or pointer low byte), then the bank
/// (or pointer high byte).
const SONG_TABLE: u16 = 0x8B91;
/// `$8B33`: triangle instrument records, `[slide][envelope lo][envelope hi][note]`.
const TRIANGLE_INSTRUMENTS: u16 = 0x8B33;
/// `$8B4B`: pulse/noise drum records, `[attack volume][attack period]` then
/// `[volume][period]`.
const DRUM_INSTRUMENTS: u16 = 0x8B4B;

/// The triangle voice.
const TRIANGLE: u8 = 2;
/// The noise voice.
const NOISE: u8 = 3;
/// `$4001` with the sweep unit off (the value the driver idles at).
const SWEEP_OFF: u8 = 0x08;
/// `$4003` length-counter bits the driver ORs into every period high byte.
const LENGTH_LOAD: u8 = 0x08;
/// The tuning a song starts with (`$067D`); the period tables assume it.
const IN_TUNE: u8 = 0x80;
/// The envelope step `$ED`/`$F1` set: decay only, never jump to the release.
const DECAY_ONLY: u8 = 0xFF;
/// `$4015` value enabling the four tone channels.
const TONE_CHANNELS: u8 = 0x0F;
/// `$4015` value enabling the tone channels and the DMC.
const ALL_CHANNELS: u8 = 0x1F;

/// Stream steps one music channel may take in a frame before the port gives up
/// on a malformed stream.
const STEP_LIMIT: usize = 2048;
/// Steps a macro walk may take before the port gives up on it.
const MACRO_STEP_LIMIT: usize = 512;
/// Events one SFX channel may read in a frame before the port gives up.
const SFX_STEP_LIMIT: usize = 1024;

/// Channel flag bits: `$0605` (music) and `$06E5` (SFX), copied to `$44` while
/// a channel runs.
mod flag {
    /// The channel is playing.
    pub(super) const ACTIVE: u8 = 0x80;
    /// Write the period high byte (`$4003`) this frame, restarting the note.
    pub(super) const RETRIGGER: u8 = 0x40;
    /// Tie notes: a new note keeps its macros running (`$E9`, cleared by `$E5`).
    pub(super) const TIE: u8 = 0x20;
    /// The period moves every frame (a period-offset macro or triangle slide),
    /// so `$4002` is rewritten.
    pub(super) const PERIOD_MACRO: u8 = 0x08;
    /// Parked by request `$FE`; request `$FD` makes it active again.
    pub(super) const PARKED: u8 = 0x04;
    /// Write `$4002` every frame and never `$4003` (`$E4`, cleared by `$E3`).
    pub(super) const PERIOD_LO_ONLY: u8 = 0x02;
    /// The drum note's one-frame attack has played; its body is next.
    pub(super) const ATTACK_DONE: u8 = 0x01;
    /// SFX: the fade-out (request `$FC`) applies to this channel.
    pub(super) const SFX_FADES: u8 = 0x01;
}

/// `$004F-$0052` bit: a higher channel already drives this voice this frame.
const VOICE_TAKEN: u8 = 0x80;
/// `$004F-$0052` bit: the voice changed hands, so its next note rewrites `$4003`.
const VOICE_RETRIGGER: u8 = flag::RETRIGGER;

/// One APU register write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Write {
    /// CPU register address (`$4000-$4017`).
    pub addr: u16,
    /// Value written.
    pub value: u8,
}

/// The low byte of a 16-bit value.
const fn lo(word: u16) -> u8 {
    word.to_le_bytes()[0]
}

/// The high byte of a 16-bit value.
const fn hi(word: u16) -> u8 {
    word.to_le_bytes()[1]
}

/// `word` with its low byte replaced.
const fn with_lo(word: u16, low: u8) -> u16 {
    u16::from_le_bytes([low, hi(word)])
}

/// Whether a pointer is set: the driver marks an unset one by a zero high byte.
const fn is_set(pointer: u16) -> bool {
    hi(pointer) != 0
}

/// Unsets a pointer by clearing its high byte; the low byte survives, as in RAM.
fn unset(pointer: &mut u16) {
    *pointer &= 0x00FF;
}

/// The volume register value that silences a voice.
const fn silent_volume(voice: u8) -> u8 {
    if voice == TRIANGLE { 0x00 } else { 0x10 }
}

/// The first register (`$4000`/`$4004`/`$4008`/`$400C`) of a voice.
fn voice_base(voice: u8) -> u16 {
    0x4000 + (u16::from(voice) << 2)
}

/// The driver bank and the song banks, read through the CPU windows.
#[derive(Clone)]
struct Banks {
    driver: Vec<u8>,
    songs: Vec<u8>,
    /// `$0600`: the song bank mapped at `$A000`.
    song_bank: u8,
}

impl Banks {
    /// Reads a driver-bank byte (CPU `$8000` window).
    fn driver_byte(&self, addr: u16) -> u8 {
        if addr < DRIVER_BASE {
            return 0;
        }
        self.driver
            .get(usize::from(addr - DRIVER_BASE))
            .copied()
            .unwrap_or(0)
    }

    /// Reads a song-stream byte by CPU address. Most songs' streams live in the
    /// `$A000` window (bank selected by `$0600`), but songs the song table marks
    /// with bit 7 (see [`Music::load_song`]) keep their header and streams in
    /// the driver bank itself, so a pointer there is in the `$8000` window — the
    /// driver reads both through the same `LDA ($45),y`.
    fn byte(&self, addr: u16) -> u8 {
        if addr < SONG_BASE {
            return self.driver_byte(addr);
        }
        let bank = self.song_bank;
        if bank < SONG_BANK_BASE {
            return 0;
        }
        let offset = usize::from(bank - SONG_BANK_BASE) * 0x2000 + usize::from(addr - SONG_BASE);
        self.songs.get(offset).copied().unwrap_or(0)
    }

    /// Reads a little-endian song-bank word.
    fn word(&self, addr: u16) -> u16 {
        u16::from_le_bytes([self.byte(addr), self.byte(addr.wrapping_add(1))])
    }

    /// A note's timer period from `$885C`/`$87FB`.
    fn period(&self, note: u8) -> u16 {
        let index = u16::from(note);
        u16::from_le_bytes([
            self.driver_byte(PERIOD_LO_TABLE + index),
            self.driver_byte(PERIOD_HI_TABLE + index),
        ])
    }
}

/// The fade-out request `$FC` starts.
#[derive(Debug, Clone, Default)]
struct Fade {
    /// `$0602`: a fade is running.
    active: bool,
    /// `$0603`: how far it has gone, 0..=15.
    step: u8,
    /// `$0604`: frames until the next step.
    timer: u8,
}

impl Fade {
    /// `$81A2`: start from full volume.
    fn start(&mut self) {
        self.active = true;
        self.timer = 1;
        self.step = 0;
    }

    /// `$81CC`: advance one frame.
    fn tick(&mut self) {
        if !self.active {
            return;
        }
        self.timer = self.timer.wrapping_sub(1);
        if self.timer != 0 {
            return;
        }
        self.timer = if self.step < 7 { 0x20 } else { 0x18 };
        if self.step == 0x0F {
            return;
        }
        self.step = self.step.wrapping_add(1);
    }

    /// `$84DF`: the faded volume register. Triangle voices are cut once the
    /// step reaches 4; envelope-mode pulses/noise switch to constant-volume zero
    /// at 5; constant-volume voices lose one volume step per fade step.
    fn scale(&self, voice: u8, volume: u8) -> u8 {
        if voice == TRIANGLE {
            if self.step >= 4 { 0x00 } else { volume }
        } else if volume & 0x10 == 0 {
            if self.step >= 5 { 0x10 } else { volume }
        } else {
            (volume & 0xF0) | (volume & 0x0F).saturating_sub(self.step)
        }
    }
}

/// Where a music channel's frame goes next (the 6502 driver's `JMP` targets).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// `$827D`: read the next stream event.
    ReadEvent,
    /// `$829B`: the note is still sounding; maybe jump to its release.
    Hold,
    /// `$82C4`: run a `$F5`-`$FF` command reached mid-note.
    MidNote,
    /// `$8328`: latch the note duration.
    LatchDuration,
    /// `$834B`: advance the stream pointer past what was read.
    Advance,
    /// `$8357`: compute the period and sweep, running the pitch macro.
    Pitch,
    /// `$840B`: run the volume envelope.
    Envelope,
    /// `$846E`: write the voice's registers.
    Registers,
    /// `$F0`: the stream ended.
    Stop,
}

/// A channel's working state for one frame: the `$44` flag copy and the `Y`
/// read offset into its stream.
struct Frame {
    flags: u8,
    offset: u16,
}

/// One music channel: a column of the RAM arrays at `$0605` (stride 8).
#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    /// `$0605`: [`flag`] bits.
    flags: u8,
    /// `$060D/$0615`: the stream read pointer.
    stream: u16,
    /// `$061D`: the hardware voice (0-1 pulse, 2 triangle, 3 noise).
    voice: u8,
    /// `$0625`: frames left in the current note.
    duration: u8,
    /// `$062D`: a fixed note length from `$F6` (0: each note carries its own).
    fixed_duration: u8,
    /// `$0635/$063D`: where `$FA` returns to after a `$FB` call.
    loop_return: u16,
    /// `$0645`: the `$FE` repeat counter.
    repeat_fe: u8,
    /// `$064D`: the `$FD` repeat counter.
    repeat_fd: u8,
    /// `$0655`: the frames-left count at which the envelope jumps to its release.
    release_at: u8,
    /// `$065D`: the release length from `$F3`-`$F5`, capped at half the note.
    release_frames: u8,
    /// `$0665`: added to every note number (`$F9`).
    transpose: u8,
    /// `$066D/$0675`: the note's timer period.
    period: u16,
    /// `$067D`: added to every looked-up period (`$F8`).
    tuning: u8,
    /// `$0685/$068D`: the volume-envelope macro; when unset, the low byte is a
    /// fixed volume.
    envelope: u16,
    /// `$0695`: the envelope macro position; 0 once it ends and decays, bit 7
    /// set to decay without the release jump.
    envelope_step: u8,
    /// `$069D`: the volume register (`$4000`) value.
    volume: u8,
    /// `$06A5`: frames between decay steps.
    decay_period: u8,
    /// `$06AD`: frames until the next decay step.
    decay_timer: u8,
    /// `$06B5/$06BD`: the pitch macro; when unset, the low byte is the fixed
    /// `$4001` sweep (triangle: the per-frame slide).
    pitch_macro: u16,
    /// `$06C5`: the pitch macro position.
    pitch_step: u8,
    /// `$06CD`: frames a new note waits before its pitch macro starts.
    pitch_delay: u8,
    /// `$06D5`: frames left of that wait.
    pitch_wait: u8,
    /// `$06DD` bit 7: the fade-out applies to this channel.
    fades: bool,
}

impl Channel {
    /// The pitch setting with no macro: sweep off, or no triangle slide.
    fn idle_pitch(&self) -> u16 {
        u16::from(if self.voice == TRIANGLE { 0 } else { SWEEP_OFF })
    }

    /// Reads a command's next operand byte.
    fn operand(&self, banks: &Banks, offset: &mut u16) -> u8 {
        let value = banks.byte(self.stream + *offset);
        *offset += 1;
        value
    }

    /// Reads a little-endian operand word at `offset`.
    fn operand_word(&self, banks: &Banks, offset: u16) -> u16 {
        u16::from_le_bytes([
            banks.byte(self.stream + offset),
            banks.byte(self.stream + offset + 1),
        ])
    }

    /// The next note's length: the fixed one if set, else a stream byte.
    fn next_duration(&self, banks: &Banks, offset: &mut u16) -> u8 {
        if self.fixed_duration != 0 {
            self.fixed_duration
        } else {
            self.operand(banks, offset)
        }
    }

    /// `$827D`: dispatch one stream event.
    fn read_event(&mut self, banks: &Banks, frame: &mut Frame) -> Step {
        let op = banks.byte(self.stream + frame.offset);
        match op {
            0x00..=0x7F => {
                self.note(banks, op, frame);
                Step::LatchDuration
            }
            0x80..=0xDF => self.instrument_note(banks, op, frame),
            0xF0 => {
                self.flags = 0;
                Step::Stop
            }
            _ => {
                frame.offset += 1;
                self.command(banks, op, frame, false)
            }
        }
    }

    /// `$829B`: mid-note, start the release once the note is down to
    /// [`Self::release_at`] frames.
    fn hold(&mut self, banks: &Banks, flags: u8) -> Step {
        let releasing = self.envelope_step & 0x80 == 0
            && self.release_at == self.duration
            && flags & flag::TIE == 0
            && is_set(self.envelope);
        if !releasing {
            return Step::MidNote;
        }
        self.envelope_step = banks.byte(self.envelope);
        Step::Pitch
    }

    /// `$82C4`: only `$F5`-`$FF` act mid-note; anything else waits.
    fn mid_note(&mut self, banks: &Banks, frame: &mut Frame) -> Step {
        let op = banks.byte(self.stream + frame.offset);
        if op < 0xF5 {
            return Step::Pitch;
        }
        frame.offset += 1;
        self.command(banks, op, frame, true)
    }

    /// `$82DD`: a `$00-$7F` note, transposed, tuned and looked up.
    fn note(&mut self, banks: &Banks, op: u8, frame: &mut Frame) {
        frame.offset = u16::from(lo(frame.offset + 1));
        let index = op.wrapping_add(self.transpose);
        self.period = if self.voice == NOISE {
            with_lo(self.period, index)
        } else {
            banks.period(index).wrapping_add(u16::from(self.tuning))
        };
        self.start_note(&mut frame.flags);
    }

    /// `$8302`: flag the note for `$4003`, restarting its macros unless tied.
    fn start_note(&mut self, flags: &mut u8) {
        let restart = *flags & flag::RETRIGGER != 0 || *flags & flag::TIE == 0;
        *flags |= flag::RETRIGGER;
        if restart {
            self.key_on();
        }
    }

    /// `$8312`: reset the macros for a new note.
    fn key_on(&mut self) {
        self.pitch_step = 0;
        self.pitch_wait = self.pitch_delay;
        self.envelope_step = 1;
        self.volume = lo(self.envelope);
    }

    /// `$8A7B`: an `$80-$DF` note played from an instrument record.
    fn instrument_note(&mut self, banks: &Banks, op: u8, frame: &mut Frame) -> Step {
        let offset = lo(frame.offset);
        let record = u16::from(op.wrapping_sub(0xC0)) * 4;
        if self.voice == TRIANGLE {
            let at = TRIANGLE_INSTRUMENTS + record;
            self.pitch_macro = u16::from(banks.driver_byte(at));
            self.envelope =
                u16::from_le_bytes([banks.driver_byte(at + 1), banks.driver_byte(at + 2)]);
            self.period = banks.period(banks.driver_byte(at + 3)).wrapping_add(0x80);
            self.release_at = 0;
            self.release_frames = 0;
            self.envelope_step = 1;
            frame.flags &= !flag::PERIOD_MACRO;
            frame.offset = u16::from(offset) + 1;
            self.start_note(&mut frame.flags);
            return Step::LatchDuration;
        }
        if frame.flags & flag::ATTACK_DONE == 0 {
            let at = DRUM_INSTRUMENTS + record;
            let volume = banks.driver_byte(at);
            self.envelope = u16::from(volume);
            self.volume = volume;
            self.period = u16::from(banks.driver_byte(at + 1));
            unset(&mut self.pitch_macro);
            frame.flags |= flag::RETRIGGER | flag::ATTACK_DONE;
            self.duration = 1;
            self.stream = self.stream.wrapping_add(u16::from(offset));
            return Step::Pitch;
        }
        let at = DRUM_INSTRUMENTS + 2 + record;
        let volume = banks.driver_byte(at);
        self.envelope = with_lo(self.envelope, volume);
        self.volume = volume;
        self.period = with_lo(self.period, banks.driver_byte(at + 1));
        frame.flags = (frame.flags | flag::RETRIGGER) & !flag::ATTACK_DONE;
        frame.offset = u16::from(offset) + 1;
        self.duration = self.next_duration(banks, &mut frame.offset).wrapping_sub(1);
        Step::Advance
    }

    /// `$8328`: latch the note length and place the release within it.
    fn latch_duration(&mut self, banks: &Banks, offset: &mut u16) -> Step {
        let length = self.next_duration(banks, offset);
        self.duration = length;
        if self.release_frames != 0 {
            self.release_at = if self.release_frames << 1 < length {
                self.release_frames
            } else {
                length >> 1
            };
        }
        Step::Advance
    }

    /// The `$E0-$FF` command handlers. Read as a new event (`$853B`), most
    /// continue reading; reached mid-note (`$8740`, `$F5`-`$FF` only), they
    /// advance the stream and let the note sound.
    fn command(&mut self, banks: &Banks, op: u8, frame: &mut Frame, mid_note: bool) -> Step {
        let Frame { flags, offset } = frame;
        let after_operand = if mid_note {
            Step::Advance
        } else {
            Step::ReadEvent
        };
        match op {
            0xE0..=0xE2 => self.pitch_delay = self.operand(banks, offset),
            0xE3 => *flags &= !flag::PERIOD_LO_ONLY,
            0xE4 => *flags |= flag::PERIOD_LO_ONLY,
            0xE5 => *flags &= !flag::TIE,
            0xE6 => {
                *flags &= !flag::PERIOD_MACRO;
                self.pitch_macro = self.idle_pitch();
            }
            0xE7 | 0xE8 => {
                if op == 0xE7 {
                    *flags |= flag::PERIOD_MACRO;
                } else {
                    *flags &= !flag::PERIOD_MACRO;
                }
                self.pitch_delay = self.operand(banks, offset);
                let low = self.operand(banks, offset);
                self.pitch_macro = u16::from_le_bytes([low, self.operand(banks, offset)]);
            }
            0xE9 => *flags |= flag::RETRIGGER | flag::TIE,
            0xEA => {
                let low = self.operand(banks, offset);
                self.envelope = u16::from_le_bytes([low, self.operand(banks, offset)]);
                self.envelope_step = 1;
            }
            0xEB => {
                *flags &= !flag::PERIOD_MACRO;
                self.pitch_macro = u16::from(self.operand(banks, offset));
            }
            0xEC => {
                self.volume = self.operand(banks, offset);
                self.envelope = u16::from(self.volume);
                self.envelope_step = 0;
            }
            0xED => {
                self.decay_period = self.operand(banks, offset);
                self.decay_timer = self.decay_period;
                self.envelope_step = DECAY_ONLY;
                return Step::LatchDuration;
            }
            0xEE => {
                self.release_frames = 0;
                self.release_at = self.operand(banks, offset);
            }
            0xEF => {
                self.release_at = 0;
                self.release_frames = 0;
            }
            0xF1 => {
                self.envelope_step = DECAY_ONLY;
                self.volume = silent_volume(self.voice);
                return Step::LatchDuration;
            }
            0xF2 => return Step::LatchDuration,
            0xF3..=0xF5 => {
                self.release_frames = self.operand(banks, offset);
                return after_operand;
            }
            0xF6 => {
                self.fixed_duration = self.operand(banks, offset);
                return after_operand;
            }
            0xF7 => {
                self.fixed_duration = 0;
                return self.skip_one_byte(mid_note);
            }
            0xF8 => {
                self.tuning = self.operand(banks, offset);
                return after_operand;
            }
            0xF9 => {
                self.transpose = self.operand(banks, offset);
                return after_operand;
            }
            _ => return self.flow_command(banks, op, offset, mid_note),
        }
        Step::ReadEvent
    }

    /// The `$FA`-`$FF` jumps: return, call, jump, repeat.
    fn flow_command(&mut self, banks: &Banks, op: u8, offset: &mut u16, mid_note: bool) -> Step {
        let after_jump = if mid_note {
            Step::Pitch
        } else {
            Step::ReadEvent
        };
        match op {
            0xFA => {
                if !is_set(self.loop_return) {
                    return self.skip_one_byte(mid_note);
                }
                self.stream = self.loop_return;
                unset(&mut self.loop_return);
                *offset = 0;
            }
            0xFB => {
                self.loop_return = self.stream.wrapping_add(*offset).wrapping_add(2);
                self.stream = self.operand_word(banks, *offset);
                *offset = 0;
            }
            0xFC => {
                self.stream = self.operand_word(banks, *offset);
                *offset = 0;
            }
            0xFD | 0xFE => return self.repeat(banks, op, offset, mid_note),
            _ => {}
        }
        after_jump
    }

    /// After a one-byte command: keep reading, or mid-note step past it.
    fn skip_one_byte(&mut self, mid_note: bool) -> Step {
        if !mid_note {
            return Step::ReadEvent;
        }
        self.stream = self.stream.wrapping_add(1);
        Step::Pitch
    }

    /// `$86DF`/`$870A` (mid-note `$87B3`/`$87D7`): `$FD`/`$FE` count, operand
    /// word: jump back until the count runs out, then step past the command.
    fn repeat(&mut self, banks: &Banks, op: u8, offset: &mut u16, mid_note: bool) -> Step {
        let counter = if op == 0xFE {
            &mut self.repeat_fe
        } else {
            &mut self.repeat_fd
        };
        if *counter == 0 {
            *counter = banks.byte(self.stream + *offset);
        } else {
            *counter -= 1;
            if *counter == 0 {
                *offset += 3;
                return if mid_note {
                    Step::Advance
                } else {
                    Step::ReadEvent
                };
            }
        }
        self.stream = self.operand_word(banks, *offset + 1);
        *offset = 0;
        if mid_note {
            Step::Pitch
        } else {
            Step::ReadEvent
        }
    }

    /// `$8357`: this frame's `$4001` sweep and timer period, after the pitch
    /// macro, its start delay, or the triangle slide.
    fn pitch(&mut self, banks: &Banks, flags: &mut u8) -> (u8, u16) {
        if is_set(self.pitch_macro) {
            if self.pitch_wait != 0 {
                self.pitch_wait -= 1;
                (SWEEP_OFF, self.period)
            } else if *flags & flag::PERIOD_MACRO == 0 {
                (self.sweep_macro(banks), self.period)
            } else {
                let low = self.period_offset_macro(banks, lo(self.period));
                (SWEEP_OFF, with_lo(self.period, low))
            }
        } else if self.voice == TRIANGLE {
            *flags |= flag::PERIOD_MACRO;
            self.slide(flags);
            (0, self.period)
        } else {
            (lo(self.pitch_macro), self.period)
        }
    }

    /// A macro of `$4001` sweep values (bit 7 set); other bytes jump to that
    /// position.
    fn sweep_macro(&mut self, banks: &Banks) -> u8 {
        let mut position = self.pitch_step;
        let mut sweep = 0;
        for _ in 0..MACRO_STEP_LIMIT {
            let entry = banks.byte(self.pitch_macro + u16::from(position));
            if entry & 0x80 != 0 {
                sweep = entry;
                break;
            }
            position = entry;
        }
        self.pitch_step = position.wrapping_add(1);
        sweep
    }

    /// A macro of period-low offsets: `$00-$7F` raise it, `$81-$FF` lower it by
    /// the low 7 bits (both clamped), `$80` jumps to the position after it.
    fn period_offset_macro(&mut self, banks: &Banks, period_lo: u8) -> u8 {
        let mut position = self.pitch_step;
        let mut moved = period_lo;
        for _ in 0..MACRO_STEP_LIMIT {
            let at = self.pitch_macro + u16::from(position);
            match banks.byte(at) {
                0x80 => position = banks.byte(at + 1),
                up @ 0x00..=0x7F => {
                    moved = period_lo.saturating_add(up);
                    break;
                }
                down => {
                    moved = period_lo.saturating_sub(down & 0x7F);
                    break;
                }
            }
        }
        self.pitch_step = position.wrapping_add(1);
        moved
    }

    /// `$8394`: the triangle slide: bit 7 of the amount slides down. Crossing a
    /// period-high boundary flags `$4003` for rewriting.
    fn slide(&mut self, flags: &mut u8) {
        let amount = lo(self.pitch_macro);
        let slid = if amount & 0x80 == 0 {
            self.period.wrapping_add(u16::from(amount))
        } else {
            self.period.wrapping_sub(u16::from(amount & 0x7F))
        };
        if hi(slid) != hi(self.period) {
            *flags |= flag::RETRIGGER;
        }
        self.period = slid;
    }

    /// `$840B`: step the envelope macro, or decay the volume once it has ended.
    /// The frame the macro ends already takes the first decay step.
    fn run_envelope(&mut self, banks: &Banks) {
        if !is_set(self.envelope) {
            return;
        }
        let step = self.envelope_step;
        if step != 0
            && step & 0x80 == 0
            && let Some(volume) = self.envelope_macro(banks)
        {
            self.volume = volume;
            return;
        }
        self.volume = self.decay(self.volume);
    }

    /// An envelope macro: nonzero bytes are volume values; a 0 is followed by
    /// either a jump position or, with bit 7, the end and its decay period.
    fn envelope_macro(&mut self, banks: &Banks) -> Option<u8> {
        let mut position = self.envelope_step;
        for _ in 0..MACRO_STEP_LIMIT {
            let at = self.envelope + u16::from(position);
            let value = banks.byte(at);
            if value != 0 {
                self.envelope_step = position.wrapping_add(1);
                return Some(value);
            }
            let next = banks.byte(at + 1);
            if next & 0x80 != 0 {
                self.decay_period = next & 0x7F;
                self.decay_timer = self.decay_period;
                self.envelope_step = 0;
                return None;
            }
            position = next;
        }
        Some(self.volume)
    }

    /// `$8445`: drop the volume one step every [`Self::decay_period`] frames.
    fn decay(&mut self, volume: u8) -> u8 {
        self.decay_timer = self.decay_timer.wrapping_sub(1);
        let level = volume & 0x0F;
        if self.decay_timer != 0 || level == 0 {
            return volume;
        }
        self.decay_timer = self.decay_period;
        (volume & 0xF0) | (level - 1)
    }
}

/// What an SFX note does to the DMC.
#[derive(Clone, Copy)]
enum SfxSound {
    /// Play the DMC record for this note.
    Sample(u8),
    /// `$F1`: stop the DMC.
    Silence,
}

/// How an SFX channel's frame ended.
enum SfxEvent {
    /// A note: it claims the DMC for its duration.
    Play(SfxSound),
    /// `$F2` (a held note) or `$F0` (the stream ended): nothing written.
    Quiet,
    /// `$FF` returns from the whole SFX engine (`$8A7A` is an `RTS`). The
    /// port stops there on `$80`-`$EF` too, which have no real handler.
    Abort,
}

/// One SFX / DMC channel: a column of the RAM arrays at `$06E5` (stride 2).
#[derive(Debug, Clone, Copy, Default)]
struct SfxChannel {
    /// `$06E5`: [`flag`] bits.
    flags: u8,
    /// `$06E7/$06E9`: the stream read pointer.
    stream: u16,
    /// `$06EB`: frames left in the current note.
    duration: u8,
    /// `$06ED`: a fixed note length from `$F4`-`$F6` (0: each note carries its
    /// own).
    fixed_duration: u8,
    /// `$06EF/$06F1`: where `$F8`-`$FA` return to after a `$FB` call.
    loop_return: u16,
    /// `$06F3`: the `$FE` repeat counter.
    repeat_fe: u8,
    /// `$06F5`: the `$FD` repeat counter.
    repeat_fd: u8,
    /// `$06F7/$06F9`: the table of four-byte DMC records (`$F3`).
    dmc_records: u16,
}

impl SfxChannel {
    /// Reads the next stream byte.
    fn operand(&self, banks: &Banks, offset: &mut u16) -> u8 {
        let value = banks.byte(self.stream + *offset);
        *offset += 1;
        value
    }

    /// `$8904`: latch the note length (fixed, or a stream byte) and advance.
    fn latch_duration(&mut self, banks: &Banks, mut offset: u16) {
        self.duration = if self.fixed_duration != 0 {
            self.fixed_duration
        } else {
            self.operand(banks, &mut offset)
        };
        self.stream = self.stream.wrapping_add(offset);
    }

    /// `$88DE`: read events up to the next note.
    fn read_events(&mut self, banks: &Banks) -> SfxEvent {
        let mut offset: u16 = 0;
        for _ in 0..SFX_STEP_LIMIT {
            let op = self.operand(banks, &mut offset);
            match op {
                0x00..=0x7F => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Play(SfxSound::Sample(op));
                }
                0xF0 => {
                    self.flags = 0;
                    return SfxEvent::Quiet;
                }
                0xF1 => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Play(SfxSound::Silence);
                }
                0xF2 => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Quiet;
                }
                0xF3 => {
                    let low = self.operand(banks, &mut offset);
                    self.dmc_records = u16::from_le_bytes([low, self.operand(banks, &mut offset)]);
                }
                0xF4..=0xF6 => self.fixed_duration = self.operand(banks, &mut offset),
                0xF7 => self.fixed_duration = 0,
                0xF8..=0xFA => {
                    if is_set(self.loop_return) {
                        self.stream = self.loop_return;
                        unset(&mut self.loop_return);
                        offset = 0;
                    }
                }
                0xFB => {
                    self.loop_return = self.stream.wrapping_add(offset).wrapping_add(2);
                    self.jump(banks, &mut offset);
                }
                0xFC => self.jump(banks, &mut offset),
                0xFD | 0xFE => self.repeat(banks, op, &mut offset),
                _ => return SfxEvent::Abort,
            }
        }
        SfxEvent::Quiet
    }

    /// `$8A0F`: continue at the operand address.
    fn jump(&mut self, banks: &Banks, offset: &mut u16) {
        self.stream = u16::from_le_bytes([
            banks.byte(self.stream + *offset),
            banks.byte(self.stream + *offset + 1),
        ]);
        *offset = 0;
    }

    /// `$8A24`/`$8A4F`: a counted loop: jump back until the count runs out,
    /// then step past the command.
    fn repeat(&mut self, banks: &Banks, op: u8, offset: &mut u16) {
        let counter = if op == 0xFE {
            &mut self.repeat_fe
        } else {
            &mut self.repeat_fd
        };
        if *counter == 0 {
            *counter = banks.byte(self.stream + *offset);
        } else {
            *counter -= 1;
            if *counter == 0 {
                *offset += 3;
                return;
            }
        }
        *offset += 1;
        self.jump(banks, offset);
    }
}

/// Writes one music voice's registers (`$846E`). `$4003` restarts the note, so
/// it is written only on a retrigger.
fn write_voice(writes: &mut Vec<Write>, voice: u8, volume: u8, sweep: u8, period: u16, flags: u8) {
    let base = voice_base(voice);
    writes.push(Write {
        addr: base,
        value: volume,
    });
    if voice != TRIANGLE {
        writes.push(Write {
            addr: base + 1,
            value: sweep,
        });
    }
    if flags & flag::RETRIGGER != 0 && flags & flag::PERIOD_LO_ONLY == 0 {
        writes.push(Write {
            addr: base + 3,
            value: hi(period) | LENGTH_LOAD,
        });
    }
    if flags & (flag::PERIOD_LO_ONLY | flag::RETRIGGER | flag::PERIOD_MACRO) != 0 {
        writes.push(Write {
            addr: base + 2,
            value: lo(period),
        });
    }
}

/// The ported music/sound driver.
#[derive(Clone)]
pub struct Music {
    banks: Banks,
    writes: Vec<Write>,
    /// `$004F-$0052`: per voice, [`VOICE_TAKEN`] and [`VOICE_RETRIGGER`].
    voices: [u8; 4],
    /// `$0040-$0043`: sound requests waiting for the next frame.
    requests: [u8; 4],
    /// `$0602-$0604`: the fade-out.
    fade: Fade,
    channels: [Channel; CHANNELS],
    sfx: [SfxChannel; SFX_CHANNELS],
}

impl Music {
    /// The driver over a PRG image: bank 10's tables and the song banks 6-7.
    #[must_use]
    pub fn from_prg(prg: &[u8]) -> Self {
        Self::new(
            prg[0x14000..0x16000].to_vec(),
            prg[0xC000..0x10000].to_vec(),
        )
    }

    /// Builds the driver from bank 10 (`driver`, 8 KiB) and banks 6-7 (`songs`, 16 KiB).
    #[must_use]
    pub fn new(driver: Vec<u8>, songs: Vec<u8>) -> Self {
        Self {
            banks: Banks {
                driver,
                songs,
                song_bank: 0,
            },
            writes: Vec::new(),
            voices: [0; 4],
            requests: [0; 4],
            fade: Fade::default(),
            channels: [Channel::default(); CHANNELS],
            sfx: [SfxChannel::default(); SFX_CHANNELS],
        }
    }

    /// Queues a sound request, as the game writes RAM `$0040-$0043`.
    pub fn request(&mut self, id: u8) {
        if let Some(slot) = self.requests.iter_mut().find(|request| **request == 0) {
            *slot = id;
        }
    }

    /// `$800C`: the driver's power-on init: clear `$0600-$06FF`, then fall
    /// through to [`Self::reset_voices`]. Returns its register writes.
    pub fn init(&mut self) -> Vec<Write> {
        let fresh = Self::new(
            std::mem::take(&mut self.banks.driver),
            std::mem::take(&mut self.banks.songs),
        );
        *self = fresh;
        self.write(0x4015, ALL_CHANNELS);
        self.write(0x4010, 0x00);
        self.stop_all();
        std::mem::take(&mut self.writes)
    }

    /// `$8020`: reset the voices and stop every channel (the game's per-scene
    /// reset). Keeps the per-voice mapping, like the ROM. Returns its register
    /// writes.
    pub fn reset_voices(&mut self) -> Vec<Write> {
        self.stop_all();
        std::mem::take(&mut self.writes)
    }

    /// `$8020`, also reached by the init and by requests `$F8`-`$FB`/`$FF`.
    fn stop_all(&mut self) {
        self.write(0x4000, 0x10);
        self.write(0x4004, 0x10);
        self.write(0x400C, 0x10);
        self.write(0x4008, 0x00);
        self.fade.active = false;
        for channel in &mut self.channels {
            channel.flags = 0;
            channel.fades = false;
        }
        for sfx in &mut self.sfx {
            sfx.flags = 0;
        }
        self.write(0x4015, TONE_CHANNELS);
    }

    /// Runs one 60 Hz frame; returns that frame's `$4000-$4017` writes in order.
    pub fn tick(&mut self) -> Vec<Write> {
        for id in std::mem::take(&mut self.requests) {
            if id != 0 {
                self.handle_request(id);
            }
        }
        self.fade.tick();
        for index in (0..CHANNELS).rev() {
            self.music_channel(index);
        }
        self.silence_untaken_voices();
        self.sfx_frame();
        std::mem::take(&mut self.writes)
    }

    /// Records one register write.
    fn write(&mut self, addr: u16, value: u8) {
        self.writes.push(Write { addr, value });
    }

    /// `$80B2`: a song id (`< $F8`) or a special command (`>= $F8`), dispatched
    /// through the `$80C7`/`$80CF` far-call table.
    fn handle_request(&mut self, id: u8) {
        match id {
            0xF8..=0xFB | 0xFF => self.stop_all(),
            0xFC => self.start_fade(),
            0xFD => self.resume_parked(),
            0xFE => self.park_active(),
            _ => self.load_song(id),
        }
    }

    /// `$81A2` (request `$FC`): fade every channel out.
    fn start_fade(&mut self) {
        self.fade.start();
        for channel in &mut self.channels {
            channel.fades = true;
        }
        for sfx in &mut self.sfx {
            sfx.flags |= flag::SFX_FADES;
        }
    }

    /// Every channel's flag byte, music then SFX.
    fn all_flags(&mut self) -> impl Iterator<Item = &mut u8> {
        let music = self.channels.iter_mut().map(|channel| &mut channel.flags);
        music.chain(self.sfx.iter_mut().map(|sfx| &mut sfx.flags))
    }

    /// `$804E` (request `$FD`): make every parked channel active again.
    fn resume_parked(&mut self) {
        self.voices = [VOICE_RETRIGGER; 4];
        for flags in self.all_flags() {
            if *flags & flag::PARKED != 0 {
                *flags |= flag::ACTIVE;
            }
        }
    }

    /// `$8080` (request `$FE`): park every active channel, then re-enable the
    /// APU's pulse/triangle/noise.
    fn park_active(&mut self) {
        self.voices = [VOICE_RETRIGGER; 4];
        for flags in self.all_flags() {
            if *flags & flag::ACTIVE != 0 {
                *flags = (*flags | flag::PARKED) & !flag::ACTIVE;
            }
        }
        self.write(0x4015, TONE_CHANNELS);
    }

    /// `$80DE`: select a song's bank, read its header, and set the channels up.
    ///
    /// The song table byte at `$8B92[X]` is the header's high byte. Bit 7 set
    /// means the pair `$8B91[X]`/`$8B92[X]` *is* the header pointer, in the
    /// driver bank's `$8000` window (the `$8105` branch) — no bank remap. That
    /// path carries ids 32 and up, whose headers start at `$8CF9` inside bank 10.
    fn load_song(&mut self, id: u8) {
        let entry = SONG_TABLE + u16::from(id) * 2;
        let low = self.banks.driver_byte(entry);
        let high = self.banks.driver_byte(entry + 1);
        if high & 0x80 != 0 {
            self.read_header(u16::from_le_bytes([low, high]));
            return;
        }
        // FME-7 bank select: only the low 4 bits reach a 128 KiB cart.
        self.banks.song_bank = high & 0x0F;
        let header = self.banks.word(SONG_BASE + u16::from(low) * 2);
        self.read_header(header);
    }

    /// `$810C`: parse a song header, a list of `[chan][voice][stream]` music
    /// channels and `[chan + 8][stream]` SFX channels ended by bit 7.
    fn read_header(&mut self, header: u16) {
        let mut offset: u16 = 0;
        let mut next = || {
            let value = self.banks.byte(header + offset);
            offset += 1;
            value
        };
        loop {
            let index = next();
            if index & 0x80 != 0 {
                return;
            }
            if index >= 8 {
                let sfx = &mut self.sfx[usize::from(index - 8)];
                let low = next();
                sfx.stream = u16::from_le_bytes([low, next()]);
                sfx.flags = flag::ACTIVE;
                sfx.duration = 1;
                sfx.fixed_duration = 0;
                sfx.repeat_fe = 0;
                sfx.repeat_fd = 0;
                unset(&mut sfx.loop_return);
                continue;
            }
            let voice = next();
            let channel = &mut self.channels[usize::from(index)];
            if voice != channel.voice {
                self.voices[usize::from(voice)] = VOICE_RETRIGGER;
            }
            channel.voice = voice;
            let low = next();
            channel.stream = u16::from_le_bytes([low, next()]);
            channel.flags = flag::ACTIVE;
            channel.tuning = IN_TUNE;
            channel.duration = 1;
            channel.release_at = 0;
            channel.fixed_duration = 0;
            channel.repeat_fe = 0;
            channel.repeat_fd = 0;
            channel.transpose = 0;
            channel.release_frames = 0;
            unset(&mut channel.loop_return);
            channel.fades = false;
            channel.pitch_macro = channel.idle_pitch();
        }
    }

    /// `$81EC`: silence the voices no channel drove this frame.
    fn silence_untaken_voices(&mut self) {
        for voice in 0..4_u8 {
            let state = std::mem::take(&mut self.voices[usize::from(voice)]);
            if state & VOICE_TAKEN == 0 {
                self.write(voice_base(voice), silent_volume(voice));
            }
        }
    }

    /// `$824D`: run one music channel's frame. Higher channels claim a voice
    /// first; a channel whose voice is taken still runs but writes nothing.
    fn music_channel(&mut self, index: usize) {
        let channel = &mut self.channels[index];
        if channel.flags & flag::ACTIVE == 0 {
            return;
        }
        let voice = channel.voice;
        let voice_before = self.voices[usize::from(voice)];
        self.voices[usize::from(voice)] = voice_before | VOICE_TAKEN;
        let mut frame = Frame {
            flags: (channel.flags & !flag::RETRIGGER) | (voice_before & VOICE_RETRIGGER),
            offset: 0,
        };
        let banks = &self.banks;
        let (mut sweep, mut period) = (0, 0);

        channel.duration = channel.duration.wrapping_sub(1);
        let mut step = if channel.duration == 0 {
            Step::ReadEvent
        } else {
            Step::Hold
        };
        let mut budget = STEP_LIMIT;
        loop {
            if budget == 0 {
                return;
            }
            budget -= 1;
            step = match step {
                Step::ReadEvent => channel.read_event(banks, &mut frame),
                Step::Hold => channel.hold(banks, frame.flags),
                Step::MidNote => channel.mid_note(banks, &mut frame),
                Step::LatchDuration => channel.latch_duration(banks, &mut frame.offset),
                Step::Advance => {
                    channel.stream = channel.stream.wrapping_add(frame.offset);
                    Step::Pitch
                }
                Step::Pitch => {
                    (sweep, period) = channel.pitch(banks, &mut frame.flags);
                    Step::Envelope
                }
                Step::Envelope => {
                    channel.run_envelope(banks);
                    Step::Registers
                }
                Step::Registers | Step::Stop => break,
            };
            if step == Step::Stop {
                break;
            }
        }
        let taken = voice_before & VOICE_TAKEN != 0;
        match step {
            Step::Stop if !taken => self.voices[usize::from(voice)] = VOICE_RETRIGGER,
            Step::Registers => {
                if !taken {
                    let volume = if self.fade.active && channel.fades {
                        self.fade.scale(voice, channel.volume)
                    } else {
                        channel.volume
                    };
                    write_voice(&mut self.writes, voice, volume, sweep, period, frame.flags);
                }
                channel.flags = frame.flags & !flag::RETRIGGER;
            }
            _ => {}
        }
    }

    /// `$88BD`: the SFX / DMC engine. Channel 1 runs first and claims the DMC
    /// while its note lasts.
    fn sfx_frame(&mut self) {
        let mut dmc_claimed = false;
        for index in (0..SFX_CHANNELS).rev() {
            let sfx = &mut self.sfx[index];
            let flags = sfx.flags;
            if flags & flag::ACTIVE == 0 {
                continue;
            }
            sfx.duration = sfx.duration.wrapping_sub(1);
            if sfx.duration != 0 {
                dmc_claimed = true;
                continue;
            }
            match sfx.read_events(&self.banks) {
                SfxEvent::Play(sound) => {
                    if !dmc_claimed {
                        let fades = flags & flag::SFX_FADES != 0;
                        self.play_sfx(self.sfx[index].dmc_records, sound, fades);
                    }
                    dmc_claimed = true;
                }
                SfxEvent::Quiet => {}
                SfxEvent::Abort => return,
            }
        }
    }

    /// `$891B`-`$8965`: load the note's DMC record and enable the channels.
    /// During a fade from step 2 on, a channel the fade applies to keeps only
    /// the tone channels enabled.
    fn play_sfx(&mut self, dmc_records: u16, sound: SfxSound, fades: bool) {
        if self.fade.active && fades && self.fade.step >= 2 {
            self.write(0x4015, TONE_CHANNELS);
            self.write(0x4015, TONE_CHANNELS);
            return;
        }
        let enable = match sound {
            SfxSound::Sample(note) => {
                let record = dmc_records + u16::from(note) * 4;
                for (reg, byte) in (0x4010..=0x4013).zip(0..) {
                    let value = self.banks.byte(record + byte);
                    self.write(reg, value);
                }
                ALL_CHANNELS
            }
            SfxSound::Silence => TONE_CHANNELS,
        };
        self.write(0x4015, enable & 0x0F);
        self.write(0x4015, enable);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// The driver over the user's ROM, or `None` (skipped) without it.
    fn assets() -> Option<Music> {
        Some(Music::from_prg(&rotj_rom::for_test()?.prg))
    }

    /// The two slices are the bytes `rotj music` extracted to `assets/audio/music/` up to
    /// 1.0.0 (`driver.bin`, `song-banks.bin`), by their FNV-1a 64.
    #[test]
    fn from_prg_takes_the_old_asset_bytes() {
        let Some(music) = assets() else {
            return;
        };
        assert_eq!(
            rotj_core::digest::fnv1a(&music.banks.driver),
            0x3780_2bcd_fc63_5b3a
        );
        assert_eq!(
            rotj_core::digest::fnv1a(&music.banks.songs),
            0x53ea_25d3_933d_49ff
        );
    }

    /// The song table selects bank 6 and header index 3 for stage 1-1's song.
    #[test]
    fn song_table_lookup() {
        let Some(mut music) = assets() else {
            return;
        };
        music.load_song(4);
        assert_eq!(music.banks.song_bank, 6);
        assert_eq!(music.banks.driver_byte(0x8B91 + 8), 3, "table index");
        assert_eq!(music.banks.word(0xA006), 0xA8C6, "header pointer");
    }

    /// Parsing the header maps four voices to 0,1,2,3 and one SFX channel.
    #[test]
    fn header_maps_voices() {
        let Some(mut music) = assets() else {
            return;
        };
        music.load_song(4);
        let voices: Vec<u8> = music.channels[0..4]
            .iter()
            .map(|channel| channel.voice)
            .collect();
        assert_eq!(voices, [0, 1, 2, 3]);
        assert_eq!(music.channels[0].stream, 0xA8DA);
        assert_eq!(music.sfx[0].flags, 0x80);
        assert_eq!(music.sfx[0].stream, 0xAF15);
    }

    /// Ids 32 and up mark its entry with bit 7 and
    /// stores the header pointer directly, in the driver bank: song `$24`'s
    /// header is `$8DE7` and maps channels 4 and 5 to voices 1 and 3.
    #[test]
    fn high_song_ids_read_their_header_from_the_driver_bank() {
        let Some(mut music) = assets() else {
            return;
        };
        music.load_song(0x24);
        assert_eq!((music.channels[4].voice, music.channels[5].voice), (1, 3));
        assert_eq!(music.channels[4].stream, 0x8DF0);
        assert_eq!(music.channels[5].stream, 0x8DF5);
        // The noise stream at `$8DF5` starts `F6 01 EC 16 0E ...`: its first
        // bytes must resolve through the `$8000` window, not the song bank.
        assert_eq!(music.banks.byte(0x8DF5), 0xF6);
        assert_eq!(music.banks.byte(0x8DF7), 0xEC);
        assert_eq!(music.banks.byte(0x8DF9), 0x0E);
    }

    /// With no song loaded the driver's per-frame loop silences all four voices.
    #[test]
    fn idle_frame_silences_the_voices() {
        let Some(mut music) = assets() else {
            return;
        };
        music.init();
        music.reset_voices();
        let writes = music.tick();
        let got: Vec<(u16, u8)> = writes
            .iter()
            .map(|write| (write.addr, write.value))
            .collect();
        assert!(
            got.contains(&(0x4000, 0x10)) && got.contains(&(0x4008, 0x00)),
            "got {got:?}"
        );
    }

    fn set_flags(music: &mut Music, flags: [u8; CHANNELS]) {
        for (channel, value) in music.channels.iter_mut().zip(flags) {
            channel.flags = value;
        }
    }

    fn flags(music: &Music) -> Vec<u8> {
        music.channels.iter().map(|channel| channel.flags).collect()
    }

    /// The `>= $F8` request commands reach the handlers the far-call table maps:
    /// `$F8`..`$FB`/`$FF` reset, `$FC` starts the fade-out, `$FD`
    /// reactivates pending channels, `$FE` parks active ones.
    #[test]
    fn special_commands_dispatch() {
        let Some(mut music) = assets() else {
            return;
        };
        music.init();

        // `$FC` ($81A2): fade on, step 0, timer 1, every `$06DD` bit 7.
        for sfx in &mut music.sfx {
            sfx.flags = 0x10;
        }
        music.handle_request(0xFC);
        assert_eq!(
            (music.fade.active, music.fade.step, music.fade.timer),
            (true, 0, 1)
        );
        assert!(music.channels.iter().all(|channel| channel.fades));
        assert!(music.sfx.iter().all(|sfx| sfx.flags & 0x01 != 0));

        // `$FD` ($804E): a channel parked with bit 2 becomes active (bit 7).
        set_flags(&mut music, [0x04, 0x80, 0x00, 0x04, 0, 0, 0, 0]);
        music.handle_request(0xFD);
        let after = flags(&music);
        assert_eq!(after[0], 0x84, "bit 2 pending channel is reactivated");
        assert_eq!(after[1], 0x80, "already-active channel is untouched");
        assert_eq!(after[3], 0x84);
        assert_eq!(
            music.voices, [0x40; 4],
            "the per-voice write flags are cleared"
        );

        // `$FE` ($8080): the inverse, and it re-enables the APU channels.
        set_flags(&mut music, [0x84, 0x80, 0x00, 0x84, 0, 0, 0, 0]);
        music.writes.clear();
        music.handle_request(0xFE);
        let after = flags(&music);
        assert_eq!(after[0], 0x04, "bit 7 is cleared, bit 2 kept");
        assert_eq!(after[1], 0x04);
        assert_eq!(after[2], 0x00);
        assert_eq!(
            music.writes,
            vec![Write {
                addr: 0x4015,
                value: 0x0F
            }]
        );

        // `$F8`..`$FB`/`$FF` ($8020): the full voice reset, which stops music.
        music.fade.active = true;
        music.channels[0].flags = 0x80;
        music.handle_request(0xFF);
        assert!(!music.fade.active);
        assert_eq!(music.channels[0].flags, 0);
        assert_eq!(
            music.writes.last().copied(),
            Some(Write {
                addr: 0x4015,
                value: 0x0F
            })
        );
    }

    /// `$84DF`: the fade step scales the written volume (constant-volume
    /// pulses lose a step each fade step; triangle/envelope mode are cut at 4/5).
    #[test]
    fn fade_volume_scaling() {
        let mut fade = Fade {
            step: 3,
            ..Fade::default()
        };
        assert_eq!(fade.scale(0, 0x1F), 0x1C);
        assert_eq!(fade.scale(0, 0x13), 0x10, "saturates at volume 0");
        assert_eq!(fade.scale(0, 0x10), 0x10, "envelope mode untouched < 5");
        assert_eq!(fade.scale(2, 0xFF), 0xFF, "triangle untouched < 4");

        fade.step = 5;
        assert_eq!(fade.scale(0, 0x1F), 0x1A);
        assert_eq!(fade.scale(0, 0x13), 0x10, "saturates at volume 0");
        assert_eq!(fade.scale(0, 0x0F), 0x10, "envelope mode -> constant 0");
        assert_eq!(fade.scale(2, 0xFF), 0x00, "triangle silenced at 4+");
    }

    /// The first frame of stage 1-1's song emits the capture's opening writes.
    #[test]
    fn first_song_frame_starts_the_channels() {
        let Some(mut music) = assets() else {
            return;
        };
        music.init();
        music.request(1);
        let _ = music.tick();
        music.reset_voices();
        music.request(4);
        let writes = music.tick();
        let got: Vec<(u16, u8)> = writes
            .iter()
            .map(|write| (write.addr, write.value))
            .collect();
        // From the reference capture, frame 610 (driver writes only).
        let expected: [(u16, u8); 13] = [
            (0x400C, 0x00),
            (0x400D, 0x08),
            (0x400F, 0x08),
            (0x400E, 0x03),
            (0x4008, 0xFF),
            (0x400B, 0x09),
            (0x400A, 0x62),
            (0x4004, 0x10),
            (0x4005, 0x08),
            (0x4000, 0x10),
            (0x4001, 0x08),
            (0x4015, 0x0F),
            (0x4015, 0x0F),
        ];
        assert_eq!(got, expected);
    }

    /// `$24` is the game's jump sound, and the driver renders it as a short
    /// noise sweep: voice 3 (noise) walks `$400E` `0E,0D,0C,0B,0A` at volume
    /// `$400C`/`$400F`, while voice 1 (pulse 2) is set up but silent (volume 0).
    /// The effect is finite — it has stopped well before frame 25.
    ///
    /// The trigger is an emulator trace: `jump` input (A at frames 750/850/950/
    /// 1050) requests `$0042=$24` at 751/851/951/1051 from bank 4 PC `$8508`,
    /// and `walkonly` (no jumps) requests nothing (`tools/emu/soundsel-jump`,
    /// `tools/emu/soundsel-walkonly`). See `docs/MUSIC.md`.
    #[test]
    fn jump_sfx_is_a_short_noise_sweep() {
        let Some(mut music) = assets() else {
            return;
        };
        music.init();
        music.request(crate::JUMP_SFX);
        let first = music.tick();
        let got: Vec<(u16, u8)> = first
            .iter()
            .map(|write| (write.addr, write.value))
            .collect();
        assert!(got.contains(&(0x400C, 0x16)), "noise volume, got {got:?}");
        assert!(
            got.contains(&(0x400E, 0x0E)),
            "first noise note, got {got:?}"
        );
        assert!(got.contains(&(0x400F, 0x08)), "noise length, got {got:?}");
        assert!(got.contains(&(0x4004, 0x10)), "silent pulse 2, got {got:?}");
        // The last noise note is at frame 14; by frame 20 it is silent.
        for _ in 0..19 {
            let _ = music.tick();
        }
        for frame in 20..40 {
            let still = music.tick().iter().any(|write| write.addr == 0x400E);
            assert!(!still, "the effect should have ended by frame {frame}");
        }
    }
}
