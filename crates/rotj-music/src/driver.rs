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

use rotj_apu::{STATUS, dmc, noise, pulse1, pulse2, status, triangle};

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

/// `$4001` with the sweep unit off (the value the driver idles at).
const SWEEP_OFF: u8 = 0x08;
/// `$4003` length-counter bits the driver ORs into every period high byte.
const LENGTH_LOAD: u8 = 0x08;
/// The tuning a song starts with (`$067D`); the period tables assume it.
const IN_TUNE: u8 = 0x80;
/// The volume register's constant-volume bit (`$4000` bit 4).
const CONSTANT_VOLUME: u8 = 0x10;
/// The volume register's level (on the triangle, low linear-counter bits).
const LEVEL: u8 = 0x0F;
/// Song table high-byte bit: the pair is a header pointer in the driver bank.
const IN_DRIVER_BANK: u8 = 0x80;
/// Song table high-byte bits that select the song bank: only these reach the FME-7 on a
/// 128 KiB cart.
const BANK_BITS: u8 = 0x0F;
/// `$82C4`: the lowest opcode that acts when reached mid-note (`CMP #$F5`).
const FIRST_MID_NOTE_OP: u8 = 0xF5;

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

/// A sign-magnitude byte: bit 7 negates the low 7 bits.
const fn signed(byte: u8) -> i8 {
    let magnitude = (byte & 0x7F).cast_signed();
    if byte & 0x80 == 0 {
        magnitude
    } else {
        -magnitude
    }
}

/// Whether a pointer is set: the driver marks an unset one by a zero high byte.
const fn is_set(pointer: u16) -> bool {
    hi(pointer) != 0
}

/// Unsets a pointer by clearing its high byte; the low byte survives, as in RAM.
fn unset(pointer: &mut u16) {
    *pointer &= 0x00FF;
}

/// A hardware voice: the APU channel a music or SFX channel drives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Voice {
    #[default]
    Pulse1,
    Pulse2,
    Triangle,
    Noise,
}

/// The registers a voice is written through.
struct VoiceRegisters {
    ctrl: u16,
    /// `None` for the triangle, which has no sweep register.
    sweep: Option<u16>,
    timer_lo: u16,
    /// The timer's high bits; the noise's length load.
    timer_hi: u16,
}

impl Voice {
    /// The voice's slot in [`Music::voices`].
    const fn index(self) -> usize {
        self as usize
    }

    /// The voice's registers (the noise voice's sweep is the ignored `$400D`).
    const fn registers(self) -> VoiceRegisters {
        match self {
            Self::Pulse1 => VoiceRegisters {
                ctrl: pulse1::CTRL,
                sweep: Some(pulse1::SWEEP),
                timer_lo: pulse1::TIMER_LO,
                timer_hi: pulse1::TIMER_HI,
            },
            Self::Pulse2 => VoiceRegisters {
                ctrl: pulse2::CTRL,
                sweep: Some(pulse2::SWEEP),
                timer_lo: pulse2::TIMER_LO,
                timer_hi: pulse2::TIMER_HI,
            },
            Self::Triangle => VoiceRegisters {
                ctrl: triangle::CTRL,
                sweep: None,
                timer_lo: triangle::TIMER_LO,
                timer_hi: triangle::TIMER_HI,
            },
            Self::Noise => VoiceRegisters {
                ctrl: noise::CTRL,
                sweep: Some(noise::UNUSED),
                timer_lo: noise::TIMER,
                timer_hi: noise::LENGTH,
            },
        }
    }

    /// The volume register value that silences the voice.
    const fn silent_volume(self) -> u8 {
        if matches!(self, Self::Triangle) {
            0x00
        } else {
            CONSTANT_VOLUME
        }
    }
}

impl TryFrom<u8> for Voice {
    type Error = u8;

    /// A header's voice byte, 0-3.
    fn try_from(byte: u8) -> Result<Self, u8> {
        match byte {
            0 => Ok(Self::Pulse1),
            1 => Ok(Self::Pulse2),
            2 => Ok(Self::Triangle),
            3 => Ok(Self::Noise),
            _ => Err(byte),
        }
    }
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
    fn scale(&self, voice: Voice, volume: u8) -> u8 {
        if voice == Voice::Triangle {
            if self.step >= 4 { 0x00 } else { volume }
        } else if volume & CONSTANT_VOLUME == 0 {
            if self.step >= 5 {
                CONSTANT_VOLUME
            } else {
                volume
            }
        } else {
            (volume & !LEVEL) | (volume & LEVEL).saturating_sub(self.step)
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

/// How a music command was reached.
#[derive(Clone, Copy)]
enum Reached {
    /// As the next event (`$853B`): the channel keeps reading.
    Event,
    /// Mid-note (`$8740`, opcodes from [`FIRST_MID_NOTE_OP`] only): the note keeps sounding.
    MidNote,
}

impl Reached {
    /// After a command that only sets state: read on, or step past it.
    const fn after_data(self) -> Step {
        match self {
            Self::Event => Step::ReadEvent,
            Self::MidNote => Step::Advance,
        }
    }

    /// After a jump: read at the new place, or let the note sound.
    const fn after_jump(self) -> Step {
        match self {
            Self::Event => Step::ReadEvent,
            Self::MidNote => Step::Pitch,
        }
    }
}

/// Which of a channel's two repeat counters a loop counts on.
#[derive(Clone, Copy)]
enum Counter {
    /// `$FD`'s.
    Fd,
    /// `$FE`'s.
    Fe,
}

impl Counter {
    /// The counter a `$FD`/`$FE` opcode counts on.
    const fn of(op: u8) -> Self {
        if op == 0xFE { Self::Fe } else { Self::Fd }
    }
}

/// A counted loop's step (`$86DF`/`$870A`, mid-note `$87B3`/`$87D7`, SFX `$8A24`/`$8A4F`): load
/// the count, else count down. Whether it jumps back.
fn repeats(counter: &mut u8, count: u8) -> bool {
    if *counter == 0 {
        *counter = count;
        return true;
    }
    *counter -= 1;
    *counter != 0
}

/// A music stream command: its opcode and fixed operands. Note lengths are read where they
/// are latched, since a fixed duration replaces them.
#[derive(Clone, Copy)]
enum Command {
    /// `$00`-`$7F`: a note.
    Note(u8),
    /// `$80`-`$DF`: a note from an instrument record, by its raw opcode.
    InstrumentNote(u8),
    /// `$E0`-`$E2`: the frames a note waits before its pitch macro.
    PitchDelay(u8),
    /// `$E4` (on) / `$E3` (off): [`flag::PERIOD_LO_ONLY`].
    PeriodLoOnly(bool),
    /// `$E5`: clear [`flag::TIE`].
    Untie,
    /// `$E6`: no pitch macro.
    IdlePitch,
    /// `$E7` (a period-offset macro, `on`) / `$E8` (a sweep macro), with its start delay.
    PeriodMacro { on: bool, delay: u8, table: u16 },
    /// `$E9`: set [`flag::RETRIGGER`] and [`flag::TIE`].
    Retrigger,
    /// `$EA`: a volume-envelope macro.
    Envelope(u16),
    /// `$EB`: a fixed sweep (triangle: slide) for [`Channel::pitch_macro`].
    PitchMacro(u8),
    /// `$EC`: a fixed volume, no envelope.
    FixedVolume(u8),
    /// `$ED`: decay only, one step per this many frames; a note length follows.
    Decay(u8),
    /// `$EE`: release at this many frames left.
    ReleaseAt(u8),
    /// `$EF`: no release.
    ClearRelease,
    /// `$F0`: the stream ends.
    Stop,
    /// `$F1`: a rest; a note length follows.
    Rest,
    /// `$F2`: hold the note; a note length follows.
    Hold,
    /// `$F3`-`$F5`: the release length.
    ReleaseFrames(u8),
    /// `$F6`: a fixed note length.
    FixedDuration(u8),
    /// `$F7`: each note carries its own length again.
    ClearFixedDuration,
    /// `$F8`: added to every looked-up period.
    Tuning(u8),
    /// `$F9`: added to every note number.
    Transpose(u8),
    /// `$FA`: return from a `$FB` call; skipped when none is open.
    Return,
    /// `$FB`: call.
    Call(u16),
    /// `$FC`: jump.
    Jump(u16),
    /// `$FD`/`$FE`: jump back to `target` until `count` runs out.
    Repeat {
        counter: Counter,
        count: u8,
        target: u16,
    },
    /// `$FF`: no handler in the ROM (its table entries point into data).
    Unhandled,
}

impl Command {
    /// The command at `stream + offset`; `offset` steps past its opcode and fixed operands.
    fn decode(banks: &Banks, stream: u16, offset: &mut u16) -> Self {
        let mut next = || {
            let value = banks.byte(stream.wrapping_add(*offset));
            *offset += 1;
            value
        };
        let op = next();
        match op {
            0x00..=0x7F => Self::Note(op),
            0x80..=0xDF => Self::InstrumentNote(op),
            0xE0..=0xE2 => Self::PitchDelay(next()),
            0xE3 | 0xE4 => Self::PeriodLoOnly(op == 0xE4),
            0xE5 => Self::Untie,
            0xE6 => Self::IdlePitch,
            0xE7 | 0xE8 => Self::PeriodMacro {
                on: op == 0xE7,
                delay: next(),
                table: u16::from_le_bytes([next(), next()]),
            },
            0xE9 => Self::Retrigger,
            0xEA => Self::Envelope(u16::from_le_bytes([next(), next()])),
            0xEB => Self::PitchMacro(next()),
            0xEC => Self::FixedVolume(next()),
            0xED => Self::Decay(next()),
            0xEE => Self::ReleaseAt(next()),
            0xEF => Self::ClearRelease,
            0xF0 => Self::Stop,
            0xF1 => Self::Rest,
            0xF2 => Self::Hold,
            0xF3..=0xF5 => Self::ReleaseFrames(next()),
            0xF6 => Self::FixedDuration(next()),
            0xF7 => Self::ClearFixedDuration,
            0xF8 => Self::Tuning(next()),
            0xF9 => Self::Transpose(next()),
            0xFA => Self::Return,
            0xFB => Self::Call(u16::from_le_bytes([next(), next()])),
            0xFC => Self::Jump(u16::from_le_bytes([next(), next()])),
            0xFD | 0xFE => Self::Repeat {
                counter: Counter::of(op),
                count: next(),
                target: u16::from_le_bytes([next(), next()]),
            },
            0xFF => Self::Unhandled,
        }
    }
}

/// An SFX stream command: its opcode and fixed operands. Note lengths are read where they
/// are latched.
#[derive(Clone, Copy)]
enum SfxCommand {
    /// `$00`-`$7F`: a DMC note.
    Note(u8),
    /// `$F0`: the stream ends.
    Stop,
    /// `$F1`: stop the DMC for a note's length.
    Silence,
    /// `$F2`: hold for a note's length.
    Hold,
    /// `$F3`: the table of four-byte DMC records.
    DmcRecords(u16),
    /// `$F4`-`$F6`: a fixed note length.
    FixedDuration(u8),
    /// `$F7`: each note carries its own length again.
    ClearFixedDuration,
    /// `$F8`-`$FA`: return from a `$FB` call; skipped when none is open.
    Return,
    /// `$FB`: call.
    Call(u16),
    /// `$FC` (`$8A0F`): jump.
    Jump(u16),
    /// `$FD`/`$FE`: jump back to `target` until `count` runs out.
    Repeat {
        counter: Counter,
        count: u8,
        target: u16,
    },
    /// `$FF` (`$8A7A`, an `RTS`) and `$80`-`$EF` (no handler): end the SFX frame.
    Abort,
}

impl SfxCommand {
    /// The command at `stream + offset`; `offset` steps past its opcode and fixed operands.
    fn decode(banks: &Banks, stream: u16, offset: &mut u16) -> Self {
        let mut next = || {
            let value = banks.byte(stream.wrapping_add(*offset));
            *offset += 1;
            value
        };
        let op = next();
        match op {
            0x00..=0x7F => Self::Note(op),
            0xF0 => Self::Stop,
            0xF1 => Self::Silence,
            0xF2 => Self::Hold,
            0xF3 => Self::DmcRecords(u16::from_le_bytes([next(), next()])),
            0xF4..=0xF6 => Self::FixedDuration(next()),
            0xF7 => Self::ClearFixedDuration,
            0xF8..=0xFA => Self::Return,
            0xFB => Self::Call(u16::from_le_bytes([next(), next()])),
            0xFC => Self::Jump(u16::from_le_bytes([next(), next()])),
            0xFD | 0xFE => Self::Repeat {
                counter: Counter::of(op),
                count: next(),
                target: u16::from_le_bytes([next(), next()]),
            },
            _ => Self::Abort,
        }
    }
}

/// `$0695`: where a channel's volume envelope is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum EnvelopeStep {
    /// The macro position, 1-`$7F`.
    At(u8),
    /// The macro ended (byte 0): the volume decays, and the release can still restart it.
    #[default]
    Decay,
    /// Bit 7: decay, never jump to the release (`$ED`, the rest).
    DecayOnly,
}

impl From<u8> for EnvelopeStep {
    fn from(byte: u8) -> Self {
        match byte {
            0 => Self::Decay,
            0x80.. => Self::DecayOnly,
            position => Self::At(position),
        }
    }
}

/// One music channel: a column of the RAM arrays at `$0605` (stride 8).
#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    /// `$0605`: [`flag`] bits.
    flags: u8,
    /// `$060D/$0615`: the stream read pointer.
    stream: u16,
    /// `$061D`: the hardware voice (0-1 pulse, 2 triangle, 3 noise).
    voice: Voice,
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
    /// `$0695`: the envelope macro position, or its decay.
    envelope_step: EnvelopeStep,
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
        u16::from(if self.voice == Voice::Triangle {
            0
        } else {
            SWEEP_OFF
        })
    }

    /// Reads a command's next operand byte.
    fn operand(&self, banks: &Banks, offset: &mut u16) -> u8 {
        let value = banks.byte(self.stream + *offset);
        *offset += 1;
        value
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
        let command = Command::decode(banks, self.stream, &mut frame.offset);
        self.run(banks, command, frame, Reached::Event)
    }

    /// `$829B`: mid-note, start the release once the note is down to
    /// [`Self::release_at`] frames.
    fn hold(&mut self, banks: &Banks, flags: u8) -> Step {
        let releasing = self.envelope_step != EnvelopeStep::DecayOnly
            && self.release_at == self.duration
            && flags & flag::TIE == 0
            && is_set(self.envelope);
        if !releasing {
            return Step::MidNote;
        }
        self.envelope_step = EnvelopeStep::from(banks.byte(self.envelope));
        Step::Pitch
    }

    /// `$82C4`: only [`FIRST_MID_NOTE_OP`] and up act mid-note; anything else waits.
    fn mid_note(&mut self, banks: &Banks, frame: &mut Frame) -> Step {
        if banks.byte(self.stream + frame.offset) < FIRST_MID_NOTE_OP {
            return Step::Pitch;
        }
        let command = Command::decode(banks, self.stream, &mut frame.offset);
        self.run(banks, command, frame, Reached::MidNote)
    }

    /// `$82DD`: a `$00-$7F` note, transposed, tuned and looked up.
    fn note(&mut self, banks: &Banks, op: u8, frame: &mut Frame) {
        frame.offset = u16::from(lo(frame.offset));
        let index = op.wrapping_add(self.transpose);
        self.period = if self.voice == Voice::Noise {
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
        self.envelope_step = EnvelopeStep::At(1);
        self.volume = lo(self.envelope);
    }

    /// `$8A7B`: an `$80-$DF` note played from an instrument record.
    fn instrument_note(&mut self, banks: &Banks, op: u8, frame: &mut Frame) -> Step {
        // The 6502's Y at the opcode.
        let offset = lo(frame.offset - 1);
        let record = u16::from(op.wrapping_sub(0xC0)) * 4;
        if self.voice == Voice::Triangle {
            let at = TRIANGLE_INSTRUMENTS + record;
            self.pitch_macro = u16::from(banks.driver_byte(at));
            self.envelope =
                u16::from_le_bytes([banks.driver_byte(at + 1), banks.driver_byte(at + 2)]);
            self.period = banks.period(banks.driver_byte(at + 3)).wrapping_add(0x80);
            self.release_at = 0;
            self.release_frames = 0;
            self.envelope_step = EnvelopeStep::At(1);
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

    /// Runs a command: the notes, and the `$E0`-`$FF` handlers read as a new event (`$853B`)
    /// or mid-note (`$8740`).
    fn run(
        &mut self,
        banks: &Banks,
        command: Command,
        frame: &mut Frame,
        reached: Reached,
    ) -> Step {
        match command {
            Command::Note(op) => {
                self.note(banks, op, frame);
                return Step::LatchDuration;
            }
            Command::InstrumentNote(op) => return self.instrument_note(banks, op, frame),
            Command::PitchDelay(delay) => self.pitch_delay = delay,
            Command::PeriodLoOnly(true) => frame.flags |= flag::PERIOD_LO_ONLY,
            Command::PeriodLoOnly(false) => frame.flags &= !flag::PERIOD_LO_ONLY,
            Command::Untie => frame.flags &= !flag::TIE,
            Command::IdlePitch => {
                frame.flags &= !flag::PERIOD_MACRO;
                self.pitch_macro = self.idle_pitch();
            }
            Command::PeriodMacro { on, delay, table } => {
                if on {
                    frame.flags |= flag::PERIOD_MACRO;
                } else {
                    frame.flags &= !flag::PERIOD_MACRO;
                }
                self.pitch_delay = delay;
                self.pitch_macro = table;
            }
            Command::Retrigger => frame.flags |= flag::RETRIGGER | flag::TIE,
            Command::Envelope(table) => {
                self.envelope = table;
                self.envelope_step = EnvelopeStep::At(1);
            }
            Command::PitchMacro(fixed) => {
                frame.flags &= !flag::PERIOD_MACRO;
                self.pitch_macro = u16::from(fixed);
            }
            Command::FixedVolume(volume) => {
                self.volume = volume;
                self.envelope = u16::from(volume);
                self.envelope_step = EnvelopeStep::Decay;
            }
            Command::Decay(period) => {
                self.decay_period = period;
                self.decay_timer = period;
                self.envelope_step = EnvelopeStep::DecayOnly;
                return Step::LatchDuration;
            }
            Command::ReleaseAt(frames) => {
                self.release_frames = 0;
                self.release_at = frames;
            }
            Command::ClearRelease => {
                self.release_at = 0;
                self.release_frames = 0;
            }
            Command::Stop => {
                self.flags = 0;
                return Step::Stop;
            }
            Command::Rest => {
                self.envelope_step = EnvelopeStep::DecayOnly;
                self.volume = self.voice.silent_volume();
                return Step::LatchDuration;
            }
            Command::Hold => return Step::LatchDuration,
            Command::ReleaseFrames(frames) => self.release_frames = frames,
            Command::FixedDuration(length) => self.fixed_duration = length,
            Command::ClearFixedDuration => self.fixed_duration = 0,
            Command::Tuning(tuning) => self.tuning = tuning,
            Command::Transpose(transpose) => self.transpose = transpose,
            Command::Return => {
                if !is_set(self.loop_return) {
                    return reached.after_data();
                }
                self.jump(self.loop_return, frame);
                unset(&mut self.loop_return);
                return reached.after_jump();
            }
            Command::Call(target) => {
                self.loop_return = self.stream.wrapping_add(frame.offset);
                self.jump(target, frame);
                return reached.after_jump();
            }
            Command::Jump(target) => {
                self.jump(target, frame);
                return reached.after_jump();
            }
            Command::Repeat {
                counter,
                count,
                target,
            } => {
                if !repeats(self.counter(counter), count) {
                    return reached.after_data();
                }
                self.jump(target, frame);
                return reached.after_jump();
            }
            Command::Unhandled => return reached.after_jump(),
        }
        reached.after_data()
    }

    /// Continues the stream at `target`.
    fn jump(&mut self, target: u16, frame: &mut Frame) {
        self.stream = target;
        frame.offset = 0;
    }

    /// The repeat counter a loop counts on.
    fn counter(&mut self, counter: Counter) -> &mut u8 {
        match counter {
            Counter::Fd => &mut self.repeat_fd,
            Counter::Fe => &mut self.repeat_fe,
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
        } else if self.voice == Voice::Triangle {
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
                offset => {
                    moved = period_lo.saturating_add_signed(signed(offset));
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
        let slid = self
            .period
            .wrapping_add_signed(i16::from(signed(lo(self.pitch_macro))));
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
        if let EnvelopeStep::At(position) = self.envelope_step
            && let Some(volume) = self.envelope_macro(banks, position)
        {
            self.volume = volume;
            return;
        }
        self.volume = self.decay(self.volume);
    }

    /// An envelope macro: nonzero bytes are volume values; a 0 is followed by
    /// either a jump position or, with bit 7, the end and its decay period.
    fn envelope_macro(&mut self, banks: &Banks, mut position: u8) -> Option<u8> {
        for _ in 0..MACRO_STEP_LIMIT {
            let at = self.envelope + u16::from(position);
            let value = banks.byte(at);
            if value != 0 {
                self.envelope_step = EnvelopeStep::from(position.wrapping_add(1));
                return Some(value);
            }
            let next = banks.byte(at + 1);
            if next & 0x80 != 0 {
                self.decay_period = next & 0x7F;
                self.decay_timer = self.decay_period;
                self.envelope_step = EnvelopeStep::Decay;
                return None;
            }
            position = next;
        }
        Some(self.volume)
    }

    /// `$8445`: drop the volume one step every [`Self::decay_period`] frames.
    fn decay(&mut self, volume: u8) -> u8 {
        self.decay_timer = self.decay_timer.wrapping_sub(1);
        let level = volume & LEVEL;
        if self.decay_timer != 0 || level == 0 {
            return volume;
        }
        self.decay_timer = self.decay_period;
        (volume & !LEVEL) | (level - 1)
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
            match SfxCommand::decode(banks, self.stream, &mut offset) {
                SfxCommand::Note(note) => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Play(SfxSound::Sample(note));
                }
                SfxCommand::Stop => {
                    self.flags = 0;
                    return SfxEvent::Quiet;
                }
                SfxCommand::Silence => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Play(SfxSound::Silence);
                }
                SfxCommand::Hold => {
                    self.latch_duration(banks, offset);
                    return SfxEvent::Quiet;
                }
                SfxCommand::DmcRecords(table) => self.dmc_records = table,
                SfxCommand::FixedDuration(length) => self.fixed_duration = length,
                SfxCommand::ClearFixedDuration => self.fixed_duration = 0,
                SfxCommand::Return => {
                    if is_set(self.loop_return) {
                        self.stream = self.loop_return;
                        unset(&mut self.loop_return);
                        offset = 0;
                    }
                }
                SfxCommand::Call(target) => {
                    self.loop_return = self.stream.wrapping_add(offset);
                    self.stream = target;
                    offset = 0;
                }
                SfxCommand::Jump(target) => {
                    self.stream = target;
                    offset = 0;
                }
                SfxCommand::Repeat {
                    counter,
                    count,
                    target,
                } => {
                    if repeats(self.counter(counter), count) {
                        self.stream = target;
                        offset = 0;
                    }
                }
                SfxCommand::Abort => return SfxEvent::Abort,
            }
        }
        SfxEvent::Quiet
    }

    /// The repeat counter a loop counts on.
    fn counter(&mut self, counter: Counter) -> &mut u8 {
        match counter {
            Counter::Fd => &mut self.repeat_fd,
            Counter::Fe => &mut self.repeat_fe,
        }
    }
}

/// Writes one music voice's registers (`$846E`). `$4003` restarts the note, so
/// it is written only on a retrigger.
fn write_voice(
    writes: &mut Vec<Write>,
    voice: Voice,
    volume: u8,
    sweep: u8,
    period: u16,
    flags: u8,
) {
    let registers = voice.registers();
    writes.push(Write {
        addr: registers.ctrl,
        value: volume,
    });
    if let Some(addr) = registers.sweep {
        writes.push(Write { addr, value: sweep });
    }
    if flags & flag::RETRIGGER != 0 && flags & flag::PERIOD_LO_ONLY == 0 {
        writes.push(Write {
            addr: registers.timer_hi,
            value: hi(period) | LENGTH_LOAD,
        });
    }
    if flags & (flag::PERIOD_LO_ONLY | flag::RETRIGGER | flag::PERIOD_MACRO) != 0 {
        writes.push(Write {
            addr: registers.timer_lo,
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
        self.write(STATUS, status::ALL);
        self.write(dmc::CTRL, 0x00);
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
        for voice in [Voice::Pulse1, Voice::Pulse2, Voice::Noise, Voice::Triangle] {
            self.write(voice.registers().ctrl, voice.silent_volume());
        }
        self.fade.active = false;
        for channel in &mut self.channels {
            channel.flags = 0;
            channel.fades = false;
        }
        for sfx in &mut self.sfx {
            sfx.flags = 0;
        }
        self.write(STATUS, status::TONE);
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
        self.write(STATUS, status::TONE);
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
        if high & IN_DRIVER_BANK != 0 {
            self.read_header(u16::from_le_bytes([low, high]));
            return;
        }
        self.banks.song_bank = high & BANK_BITS;
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
            let voice = Voice::try_from(next()).expect("a header's voice is 0-3");
            let channel = &mut self.channels[usize::from(index)];
            if voice != channel.voice {
                self.voices[voice.index()] = VOICE_RETRIGGER;
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
        for voice in [Voice::Pulse1, Voice::Pulse2, Voice::Triangle, Voice::Noise] {
            let state = std::mem::take(&mut self.voices[voice.index()]);
            if state & VOICE_TAKEN == 0 {
                self.write(voice.registers().ctrl, voice.silent_volume());
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
        let voice_before = self.voices[voice.index()];
        self.voices[voice.index()] = voice_before | VOICE_TAKEN;
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
            Step::Stop if !taken => self.voices[voice.index()] = VOICE_RETRIGGER,
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
            self.write(STATUS, status::TONE);
            self.write(STATUS, status::TONE);
            return;
        }
        let enable = match sound {
            SfxSound::Sample(note) => {
                let record = dmc_records + u16::from(note) * 4;
                for (reg, byte) in (dmc::CTRL..=dmc::LENGTH).zip(0..) {
                    let value = self.banks.byte(record + byte);
                    self.write(reg, value);
                }
                status::ALL
            }
            SfxSound::Silence => status::TONE,
        };
        self.write(STATUS, enable & status::TONE);
        self.write(STATUS, enable);
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
        let voices: Vec<Voice> = music.channels[0..4]
            .iter()
            .map(|channel| channel.voice)
            .collect();
        assert_eq!(
            voices,
            [Voice::Pulse1, Voice::Pulse2, Voice::Triangle, Voice::Noise]
        );
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
        assert_eq!(
            (music.channels[4].voice, music.channels[5].voice),
            (Voice::Pulse2, Voice::Noise)
        );
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
            got.contains(&(pulse1::CTRL, 0x10)) && got.contains(&(triangle::CTRL, 0x00)),
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
                addr: STATUS,
                value: status::TONE
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
                addr: STATUS,
                value: status::TONE
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
        assert_eq!(fade.scale(Voice::Pulse1, 0x1F), 0x1C);
        assert_eq!(
            fade.scale(Voice::Pulse1, 0x13),
            0x10,
            "saturates at volume 0"
        );
        assert_eq!(
            fade.scale(Voice::Pulse1, 0x10),
            0x10,
            "envelope mode untouched < 5"
        );
        assert_eq!(
            fade.scale(Voice::Triangle, 0xFF),
            0xFF,
            "triangle untouched < 4"
        );

        fade.step = 5;
        assert_eq!(fade.scale(Voice::Pulse1, 0x1F), 0x1A);
        assert_eq!(
            fade.scale(Voice::Pulse1, 0x13),
            0x10,
            "saturates at volume 0"
        );
        assert_eq!(
            fade.scale(Voice::Pulse1, 0x0F),
            0x10,
            "envelope mode -> constant 0"
        );
        assert_eq!(
            fade.scale(Voice::Triangle, 0xFF),
            0x00,
            "triangle silenced at 4+"
        );
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
            (noise::CTRL, 0x00),
            (noise::UNUSED, 0x08),
            (noise::LENGTH, 0x08),
            (noise::TIMER, 0x03),
            (triangle::CTRL, 0xFF),
            (triangle::TIMER_HI, 0x09),
            (triangle::TIMER_LO, 0x62),
            (pulse2::CTRL, 0x10),
            (pulse2::SWEEP, 0x08),
            (pulse1::CTRL, 0x10),
            (pulse1::SWEEP, 0x08),
            (STATUS, status::TONE),
            (STATUS, status::TONE),
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
        assert!(
            got.contains(&(noise::CTRL, 0x16)),
            "noise volume, got {got:?}"
        );
        assert!(
            got.contains(&(noise::TIMER, 0x0E)),
            "first noise note, got {got:?}"
        );
        assert!(
            got.contains(&(noise::LENGTH, 0x08)),
            "noise length, got {got:?}"
        );
        assert!(
            got.contains(&(pulse2::CTRL, 0x10)),
            "silent pulse 2, got {got:?}"
        );
        // The last noise note is at frame 14; by frame 20 it is silent.
        for _ in 0..19 {
            let _ = music.tick();
        }
        for frame in 20..40 {
            let still = music.tick().iter().any(|write| write.addr == noise::TIMER);
            assert!(!still, "the effect should have ended by frame {frame}");
        }
    }

    /// A banks whose driver window holds `stream` from `$8000`, padded with zeros.
    fn synthetic(stream: &[u8]) -> Banks {
        let mut driver = stream.to_vec();
        driver.resize(0x40, 0);
        Banks {
            driver,
            songs: Vec::new(),
            song_bank: 0,
        }
    }

    fn music_channel() -> Channel {
        Channel {
            stream: DRIVER_BASE,
            ..Channel::default()
        }
    }

    fn name(command: Command) -> &'static str {
        match command {
            Command::Note(_) => "Note",
            Command::InstrumentNote(_) => "InstrumentNote",
            Command::PitchDelay(_) => "PitchDelay",
            Command::PeriodLoOnly(_) => "PeriodLoOnly",
            Command::Untie => "Untie",
            Command::IdlePitch => "IdlePitch",
            Command::PeriodMacro { .. } => "PeriodMacro",
            Command::Retrigger => "Retrigger",
            Command::Envelope(_) => "Envelope",
            Command::PitchMacro(_) => "PitchMacro",
            Command::FixedVolume(_) => "FixedVolume",
            Command::Decay(_) => "Decay",
            Command::ReleaseAt(_) => "ReleaseAt",
            Command::ClearRelease => "ClearRelease",
            Command::Stop => "Stop",
            Command::Rest => "Rest",
            Command::Hold => "Hold",
            Command::ReleaseFrames(_) => "ReleaseFrames",
            Command::FixedDuration(_) => "FixedDuration",
            Command::ClearFixedDuration => "ClearFixedDuration",
            Command::Tuning(_) => "Tuning",
            Command::Transpose(_) => "Transpose",
            Command::Return => "Return",
            Command::Call(_) => "Call",
            Command::Jump(_) => "Jump",
            Command::Repeat { .. } => "Repeat",
            Command::Unhandled => "Unhandled",
        }
    }

    /// The variant and bytes consumed (opcode included) per opcode, from the pre-refactor driver.
    fn music_table(op: u8) -> (&'static str, u16) {
        match op {
            0x00..=0x7F => ("Note", 1),
            0x80..=0xDF => ("InstrumentNote", 1),
            0xE0..=0xE2 => ("PitchDelay", 2),
            0xE3 | 0xE4 => ("PeriodLoOnly", 1),
            0xE5 => ("Untie", 1),
            0xE6 => ("IdlePitch", 1),
            0xE7 | 0xE8 => ("PeriodMacro", 4),
            0xE9 => ("Retrigger", 1),
            0xEA => ("Envelope", 3),
            0xEB => ("PitchMacro", 2),
            0xEC => ("FixedVolume", 2),
            0xED => ("Decay", 2),
            0xEE => ("ReleaseAt", 2),
            0xEF => ("ClearRelease", 1),
            0xF0 => ("Stop", 1),
            0xF1 => ("Rest", 1),
            0xF2 => ("Hold", 1),
            0xF3..=0xF5 => ("ReleaseFrames", 2),
            0xF6 => ("FixedDuration", 2),
            0xF7 => ("ClearFixedDuration", 1),
            0xF8 => ("Tuning", 2),
            0xF9 => ("Transpose", 2),
            0xFA => ("Return", 1),
            0xFB => ("Call", 3),
            0xFC => ("Jump", 3),
            0xFD | 0xFE => ("Repeat", 4),
            0xFF => ("Unhandled", 1),
        }
    }

    #[test]
    fn music_decode_table_covers_every_opcode() {
        for op in 0..=0xFF {
            let banks = synthetic(&[op, 0x11, 0x22, 0x33, 0x44]);
            let mut offset = 0;
            let command = Command::decode(&banks, DRIVER_BASE, &mut offset);
            assert_eq!((name(command), offset), music_table(op), "opcode {op:#04X}");
        }
    }

    fn sfx_name(command: SfxCommand) -> &'static str {
        match command {
            SfxCommand::Note(_) => "Note",
            SfxCommand::Stop => "Stop",
            SfxCommand::Silence => "Silence",
            SfxCommand::Hold => "Hold",
            SfxCommand::DmcRecords(_) => "DmcRecords",
            SfxCommand::FixedDuration(_) => "FixedDuration",
            SfxCommand::ClearFixedDuration => "ClearFixedDuration",
            SfxCommand::Return => "Return",
            SfxCommand::Call(_) => "Call",
            SfxCommand::Jump(_) => "Jump",
            SfxCommand::Repeat { .. } => "Repeat",
            SfxCommand::Abort => "Abort",
        }
    }

    fn sfx_table(op: u8) -> (&'static str, u16) {
        match op {
            0x00..=0x7F => ("Note", 1),
            0xF0 => ("Stop", 1),
            0xF1 => ("Silence", 1),
            0xF2 => ("Hold", 1),
            0xF3 => ("DmcRecords", 3),
            0xF4..=0xF6 => ("FixedDuration", 2),
            0xF7 => ("ClearFixedDuration", 1),
            0xF8..=0xFA => ("Return", 1),
            0xFB => ("Call", 3),
            0xFC => ("Jump", 3),
            0xFD | 0xFE => ("Repeat", 4),
            0x80..=0xEF | 0xFF => ("Abort", 1),
        }
    }

    #[test]
    fn sfx_decode_table_covers_every_opcode() {
        for op in 0..=0xFF {
            let banks = synthetic(&[op, 0x11, 0x22, 0x33, 0x44]);
            let mut offset = 0;
            let command = SfxCommand::decode(&banks, DRIVER_BASE, &mut offset);
            assert_eq!(
                (sfx_name(command), offset),
                sfx_table(op),
                "opcode {op:#04X}"
            );
        }
    }

    #[test]
    fn decode_near_the_end_of_the_address_space_wraps() {
        let banks = synthetic(&[]);
        let mut offset = 0;
        let _ = Command::decode(&banks, 0xFFFF, &mut offset);
        let mut offset = 0;
        let _ = SfxCommand::decode(&banks, 0xFFFF, &mut offset);
    }

    #[test]
    fn pitch_delay_commands_set_the_delay() {
        for op in 0xE0..=0xE2 {
            let banks = synthetic(&[op, 0x05]);
            let mut channel = music_channel();
            let mut frame = Frame {
                flags: 0,
                offset: 0,
            };
            assert!(channel.read_event(&banks, &mut frame) == Step::ReadEvent);
            assert_eq!(
                (channel.pitch_delay, frame.offset),
                (5, 2),
                "opcode {op:#04X}"
            );
        }
    }

    #[test]
    fn period_lo_only_is_cleared_by_e3_and_set_by_e4() {
        let mut channel = music_channel();
        let mut frame = Frame {
            flags: flag::PERIOD_LO_ONLY,
            offset: 0,
        };
        assert!(channel.read_event(&synthetic(&[0xE3]), &mut frame) == Step::ReadEvent);
        assert_eq!((frame.flags, frame.offset), (0, 1));
        let mut frame = Frame {
            flags: 0,
            offset: 0,
        };
        assert!(channel.read_event(&synthetic(&[0xE4]), &mut frame) == Step::ReadEvent);
        assert_eq!((frame.flags, frame.offset), (flag::PERIOD_LO_ONLY, 1));
    }

    #[test]
    fn e8_clears_the_period_macro_and_loads_delay_and_table() {
        let banks = synthetic(&[0xE8, 0x11, 0x22, 0x33]);
        let mut channel = music_channel();
        let mut frame = Frame {
            flags: flag::PERIOD_MACRO | flag::TIE,
            offset: 0,
        };
        assert!(channel.read_event(&banks, &mut frame) == Step::ReadEvent);
        assert_eq!(frame.flags, flag::TIE);
        assert_eq!(frame.offset, 4);
        assert_eq!((channel.pitch_delay, channel.pitch_macro), (0x11, 0x3322));
    }

    #[test]
    fn release_length_commands_set_it() {
        for op in 0xF3..=0xF5 {
            let banks = synthetic(&[op, 0x07]);
            let mut channel = music_channel();
            let mut frame = Frame {
                flags: 0,
                offset: 0,
            };
            assert!(channel.read_event(&banks, &mut frame) == Step::ReadEvent);
            assert_eq!(
                (channel.release_frames, frame.offset),
                (7, 2),
                "opcode {op:#04X}"
            );
        }
    }

    #[test]
    fn f7_clears_the_fixed_duration() {
        let mut channel = music_channel();
        channel.fixed_duration = 9;
        let mut frame = Frame {
            flags: 0,
            offset: 0,
        };
        assert!(channel.read_event(&synthetic(&[0xF7]), &mut frame) == Step::ReadEvent);
        assert_eq!((channel.fixed_duration, frame.offset), (0, 1));
    }

    #[test]
    fn ff_skips_its_byte_as_an_event_and_changes_nothing_mid_note() {
        let banks = synthetic(&[0xFF]);
        let mut channel = music_channel();
        let mut frame = Frame {
            flags: 0,
            offset: 0,
        };
        assert!(channel.read_event(&banks, &mut frame) == Step::ReadEvent);
        assert_eq!((channel.stream, frame.offset), (DRIVER_BASE, 1));
        let mut frame = Frame {
            flags: 0,
            offset: 0,
        };
        assert!(channel.mid_note(&banks, &mut frame) == Step::Pitch);
        assert_eq!(channel.stream, DRIVER_BASE);
    }

    #[test]
    fn sfx_fixed_duration_commands_set_it() {
        for op in 0xF4..=0xF6 {
            let banks = synthetic(&[op, 0x09, 0xF0]);
            let mut channel = SfxChannel {
                stream: DRIVER_BASE,
                ..SfxChannel::default()
            };
            assert!(matches!(channel.read_events(&banks), SfxEvent::Quiet));
            assert_eq!(channel.fixed_duration, 9, "opcode {op:#04X}");
        }
    }

    #[test]
    fn sfx_return_opcodes_all_return_from_a_call() {
        for op in 0xF8..=0xFA {
            let mut bytes = vec![op];
            bytes.resize(0x10, 0);
            bytes.push(0xF0);
            let banks = synthetic(&bytes);
            let mut channel = SfxChannel {
                stream: DRIVER_BASE,
                loop_return: DRIVER_BASE + 0x10,
                ..SfxChannel::default()
            };
            assert!(matches!(channel.read_events(&banks), SfxEvent::Quiet));
            assert_eq!(channel.stream, DRIVER_BASE + 0x10, "opcode {op:#04X}");
            assert!(!is_set(channel.loop_return), "opcode {op:#04X}");
        }
    }

    #[test]
    fn sfx_abort_opcodes_end_the_frame() {
        for op in (0x80..=0xEF).chain([0xFF]) {
            let banks = synthetic(&[op]);
            let mut channel = SfxChannel {
                stream: DRIVER_BASE,
                ..SfxChannel::default()
            };
            assert!(matches!(channel.read_events(&banks), SfxEvent::Abort));
            assert_eq!(channel.stream, DRIVER_BASE, "opcode {op:#04X}");
        }
    }

    /// `$0695`: 0 decays, bit 7 decays without the release, any other byte is a position.
    #[test]
    fn envelope_step_from_every_byte() {
        for byte in 0..=u8::MAX {
            let step = EnvelopeStep::from(byte);
            assert_eq!(step == EnvelopeStep::Decay, byte == 0, "{byte:#04X}");
            assert_eq!(
                step == EnvelopeStep::DecayOnly,
                byte & 0x80 != 0,
                "{byte:#04X}"
            );
            if let EnvelopeStep::At(position) = step {
                assert_eq!(position, byte);
            }
        }
    }

    /// `signed` matches the slide's and the period-offset macro's two-branch forms.
    #[test]
    fn signed_matches_sign_magnitude() {
        for byte in 0..=u8::MAX {
            let amount = signed(byte);
            for period in [0x0000, 0x00FF, 0x0100, 0x07F0, 0xFFFF] {
                let slid = if byte & 0x80 == 0 {
                    u16::wrapping_add(period, u16::from(byte))
                } else {
                    u16::wrapping_sub(period, u16::from(byte & 0x7F))
                };
                assert_eq!(period.wrapping_add_signed(i16::from(amount)), slid);
            }
            for period_lo in 0..=u8::MAX {
                let moved = if byte & 0x80 == 0 {
                    period_lo.saturating_add(byte)
                } else {
                    period_lo.saturating_sub(byte & 0x7F)
                };
                assert_eq!(period_lo.saturating_add_signed(amount), moved);
            }
        }
    }

    /// An envelope stepping past position `$7F` decays without the release, as the ROM's
    /// byte reaching `$80` does.
    #[test]
    fn envelope_past_position_7f_decays_only() {
        let mut stream = vec![0x10; 0x100];
        stream[0x7F] = 0x37;
        stream[0x80] = 0x3F;
        let banks = Banks {
            driver: stream,
            songs: Vec::new(),
            song_bank: 0,
        };
        let mut channel = Channel {
            envelope: DRIVER_BASE,
            envelope_step: EnvelopeStep::At(0x7F),
            decay_period: 1,
            decay_timer: 1,
            ..music_channel()
        };
        channel.run_envelope(&banks);
        assert_eq!(channel.volume, 0x37);
        assert_eq!(channel.envelope_step, EnvelopeStep::DecayOnly);
        channel.run_envelope(&banks);
        assert_eq!(channel.volume, 0x36, "decays, does not step to $80");
        assert!(channel.hold(&banks, 0) == Step::MidNote, "no release");
    }

    /// A release position byte with bit 7 releases once, then blocks the next release.
    #[test]
    fn release_byte_with_bit_7_releases_once() {
        let banks = synthetic(&[0x90]);
        let mut channel = Channel {
            envelope: DRIVER_BASE,
            envelope_step: EnvelopeStep::At(5),
            release_at: 3,
            duration: 3,
            ..music_channel()
        };
        assert!(channel.hold(&banks, 0) == Step::Pitch, "releases");
        assert_eq!(channel.envelope_step, EnvelopeStep::DecayOnly);
        assert!(
            channel.hold(&banks, 0) == Step::MidNote,
            "no second release"
        );
    }
}
