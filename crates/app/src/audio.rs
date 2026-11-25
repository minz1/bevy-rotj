//! Audio output: the port's 2A03 synth, played through Bevy's audio engine.
//!
//! Music advances on the game's own 60 Hz clock — a fixed number of APU ticks
//! per frame, exactly like the console — and the mixed samples land in a small
//! queue the audio device drains. Keeping the synth on the game clock is what
//! makes playback deterministic and reproducible: what you hear is a function of
//! the frame count, not of the sound card's timing.
//!
//! The soundtrack comes from the *ported* sound driver (`rotj-music`), fed by
//! the extracted driver and song banks: each game frame ticks the driver, which
//! returns the APU register writes the game itself would make
//! (`docs/re/MUSIC.md`, `crates/rotj-music/tests/acceptance.rs`).
//!
//! The game's own requests (`$40-$43`, from `Game::nmi`) drive it from power-on: the
//! title's song `$01` (`$61AE`), each stage's `$D980[$0400 / 2]` (`$D973`) and every
//! effect.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::audio::{
    AddAudioSource, AudioPlayer, ChannelCount, Decodable, Sample, SampleRate, Source,
};
use bevy::prelude::*;
use rotj_apu::stream::ApuStream;
use rotj_core::SoundId;
use rotj_music::{SoundKind, classify_request, is_stop_all};

use crate::settings::SfxVoices;

/// Maximum simultaneous polyphonic SFX voices when [`SfxVoices::Unlimited`] is active.
const MAX_SFX_VOICES: usize = 16;
/// Extra 60 Hz frames an idle SFX APU rings out so its output filter settles smoothly to 0.
const SFX_TAIL_FRAMES: u8 = 4;

/// Output sample rate.
const SAMPLE_RATE: u32 = 44_100;
/// Samples a frame of the app's 60 Hz game clock is worth at [`SAMPLE_RATE`].
const SAMPLES_PER_FRAME: usize = 735;
/// How much mixed audio stays queued, in samples (a quarter second).
const QUEUE_CAPACITY: usize = (SAMPLE_RATE / 4) as usize;
/// The queue the frame size steers toward and playback waits for: three frames, enough
/// to ride out the game clock's jitter against the device's.
const QUEUE_TARGET: usize = 3 * SAMPLES_PER_FRAME;
/// How many samples a frame may run over or under [`SAMPLES_PER_FRAME`].
const NUDGE: usize = 2;
const _: () = assert!(SAMPLES_PER_FRAME * 60 == SAMPLE_RATE as usize);
/// How far the applied gain moves per sample: full to silent in 10 ms.
const FADE_STEP: f32 = 1.0 / 441.0;

/// The gain for a 0-100 volume, squared so the steps sound even.
#[must_use]
pub(crate) fn gain(volume: u8) -> f32 {
    (f32::from(volume) / 100.0).powi(2)
}

/// Samples for the next frame: a frame's worth, [`NUDGE`] more or fewer while the queue
/// is half a frame off [`QUEUE_TARGET`], so production follows the device's clock.
fn samples_for(queued: usize) -> usize {
    if queued + SAMPLES_PER_FRAME / 2 < QUEUE_TARGET {
        SAMPLES_PER_FRAME + NUDGE
    } else if queued > QUEUE_TARGET + SAMPLES_PER_FRAME / 2 {
        SAMPLES_PER_FRAME - NUDGE
    } else {
        SAMPLES_PER_FRAME
    }
}

/// The 2A03 with the DMC's sample bank (the fixed `$E000-$FFFF` window), without
/// which the DMC reads zeros and the bass and drums samples are silent.
fn apu_with_samples(prg: &[u8]) -> rotj_apu::apu::Apu {
    let mut apu = rotj_apu::apu::Apu::new();
    apu.set_sample_rom(rotj_apu::dmc::SampleRom::new(
        0xE000,
        rotj_core::prg::bank(prg, 15)
            .expect("PRG has fixed bank 15")
            .to_vec(),
    ));
    apu
}

/// The app's driver at its power-on init, for tests that feed it one request.
#[cfg(test)]
pub(crate) fn driver() -> rotj_music::Music {
    let mut driver = rotj_music::Music::from_prg(&rotj_rom::for_test().expect("the ROM").prg);
    driver.init();
    driver
}

/// Mixed samples in flight, from the game clock to the audio device.
#[derive(Resource, Clone, Default)]
pub(crate) struct SampleQueue(Arc<Mutex<VecDeque<f32>>>);

impl SampleQueue {
    /// Queues one sample, dropping it if the device is not keeping up (stale
    /// audio is worse than a dropped sample).
    fn push(&self, sample: f32) {
        if let Ok(mut queue) = self.0.lock()
            && queue.len() < QUEUE_CAPACITY
        {
            queue.push_back(sample);
        }
    }

    /// Takes one sample, if any is queued.
    fn pop(&self) -> Option<f32> {
        self.0.lock().ok()?.pop_front()
    }

    /// Samples currently queued.
    fn len(&self) -> usize {
        self.0.lock().map_or(0, |queue| queue.len())
    }
}

/// Whether the game clock is driving the sound driver or frozen under the menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AudioDrive {
    /// Ticks the music driver and ramps gain toward `target_gain`.
    Running {
        /// Target output gain (0.0..=1.0).
        target_gain: f32,
    },
    /// Holds the music driver still and ramps gain toward silence.
    Frozen,
}

/// Transparent soft limiter: identity inside `[-0.85, 0.85]`, smooth asymptotic ceiling
/// approaching `±1.0` so simultaneous SFX bursts never hard-clip.
#[must_use]
fn soft_clip(sample: f32) -> f32 {
    const KNEE: f32 = 0.85;
    const HEADROOM: f32 = 1.0 - KNEE;
    let abs = sample.abs();
    if abs <= KNEE {
        sample
    } else {
        sample.signum() * (KNEE + HEADROOM * ((abs - KNEE) / HEADROOM).tanh())
    }
}

/// One dedicated SFX voice in the [`SfxVoices::Unlimited`] polyphonic pool.
struct SfxVoice {
    stream: ApuStream,
    driver: rotj_music::Music,
    active: bool,
    tail_frames: u8,
    age: u64,
}

impl SfxVoice {
    fn new(prg_bank15: &[u8], template: &rotj_music::Music) -> Self {
        let mut apu = rotj_apu::apu::Apu::new();
        apu.set_sample_rom(rotj_apu::dmc::SampleRom::new(0xE000, prg_bank15.to_vec()));
        let mut stream = ApuStream::new(apu, SAMPLE_RATE);
        let mut driver = template.clone();
        for write in driver.init() {
            stream.apu_mut().write(write.addr, write.value);
        }
        Self {
            stream,
            driver,
            active: false,
            tail_frames: 0,
            age: 0,
        }
    }

    fn trigger(&mut self, id: u8, age: u64) {
        for write in self.driver.init() {
            self.stream.apu_mut().write(write.addr, write.value);
        }
        self.driver.request(id);
        self.active = true;
        self.tail_frames = SFX_TAIL_FRAMES;
        self.age = age;
    }

    fn silence(&mut self) {
        for write in self.driver.init() {
            self.stream.apu_mut().write(write.addr, write.value);
        }
        self.active = false;
        self.tail_frames = 0;
    }

    fn tick_and_mix(&mut self, mix: &mut [f32]) {
        for write in self.driver.tick() {
            self.stream.apu_mut().write(write.addr, write.value);
        }
        let mut scratch = [0.0_f32; SAMPLES_PER_FRAME + NUDGE];
        let buf = &mut scratch[..mix.len()];
        self.stream.fill(buf);
        for (dst, &src) in mix.iter_mut().zip(buf.iter()) {
            *dst += src;
        }
        if self.driver.is_idle() {
            self.tail_frames = self.tail_frames.saturating_sub(1);
            if self.tail_frames == 0 {
                self.active = false;
            }
        } else {
            self.tail_frames = SFX_TAIL_FRAMES;
        }
    }
}

/// The synth on the game's clock: ticks the ported driver each frame, applies
/// the register writes it returns, and fills the queue.
#[derive(Resource)]
pub(crate) struct Music {
    stream: ApuStream,
    driver: rotj_music::Music,
    queue: SampleQueue,
    /// Fixed bank 15 sample ROM bytes, shared when allocating polyphonic SFX voices.
    prg_bank15: Vec<u8>,
    /// Fresh driver template cloned when growing the SFX voice pool.
    driver_template: rotj_music::Music,
    /// Polyphonic SFX pool used when [`SfxVoices::Unlimited`] is enabled.
    sfx_pool: Vec<SfxVoice>,
    /// Monotonic counter for oldest-voice stealing when all [`MAX_SFX_VOICES`] are busy.
    sfx_seq: u64,
    /// Per-tick sound requests queued by `Step::Game` for 1-to-1 lockstep on multi-tick catch-up.
    pending_frames: VecDeque<[Option<SoundId>; 4]>,
    /// The gain applied to the last sample, ramping toward the asked one.
    gain: f32,
}

impl Music {
    /// Builds the synth, loads the driver and runs its power-on init (bank 10 `$8000`,
    /// from `$D394`) so requests queued on frame 0 are never overwritten.
    #[must_use]
    pub fn new(queue: SampleQueue, prg: &[u8]) -> Self {
        let prg_bank15 = rotj_core::prg::bank(prg, 15)
            .expect("PRG has fixed bank 15")
            .to_vec();
        let driver_template = rotj_music::Music::from_prg(prg);
        let mut stream = ApuStream::new(apu_with_samples(prg), SAMPLE_RATE);
        let mut driver = driver_template.clone();
        for write in driver.init() {
            stream.apu_mut().write(write.addr, write.value);
        }
        Self {
            stream,
            driver,
            queue,
            prg_bank15,
            driver_template,
            sfx_pool: Vec::new(),
            sfx_seq: 0,
            pending_frames: VecDeque::new(),
            gain: 0.0,
        }
    }

    /// Queues one request directly onto the primary Classic driver (`$40-$43`).
    #[cfg(test)]
    pub fn request(&mut self, id: SoundId) {
        self.driver.request(id.byte());
    }

    /// Routes one request byte according to `voices`: Classic sends everything to the primary
    /// driver; Unlimited isolates SFX onto the polyphonic voice pool so music channels are
    /// never interrupted.
    fn route_request(&mut self, id: u8, voices: SfxVoices) {
        if voices == SfxVoices::Classic {
            self.driver.request(id);
            return;
        }
        match classify_request(id) {
            SoundKind::Music => self.driver.request(id),
            SoundKind::Command => {
                self.driver.request(id);
                if is_stop_all(id) {
                    for voice in &mut self.sfx_pool {
                        voice.silence();
                    }
                }
            }
            SoundKind::Sfx => self.spawn_sfx(id),
        }
    }

    fn spawn_sfx(&mut self, id: u8) {
        self.sfx_seq = self.sfx_seq.wrapping_add(1);
        let age = self.sfx_seq;
        if let Some(voice) = self.sfx_pool.iter_mut().find(|v| !v.active) {
            voice.trigger(id, age);
            return;
        }
        if self.sfx_pool.len() < MAX_SFX_VOICES {
            let mut voice = SfxVoice::new(&self.prg_bank15, &self.driver_template);
            voice.trigger(id, age);
            self.sfx_pool.push(voice);
            return;
        }
        if let Some(oldest) = self.sfx_pool.iter_mut().min_by_key(|v| v.age) {
            oldest.trigger(id, age);
        }
    }

    /// Queues a tick's four `$40-$43` requests so multi-tick catch-up steps audio 1-to-1.
    pub fn request_frame(&mut self, sounds: [Option<SoundId>; 4]) {
        self.pending_frames.push_back(sounds);
    }

    /// Advances one game frame in Classic voice mode (test convenience).
    #[cfg(test)]
    pub fn advance_frame(&mut self, drive: AudioDrive) {
        self.advance_frame_with(drive, SfxVoices::Classic);
    }

    /// Advances one game frame: ticks the primary driver (and any active SFX voices) when
    /// [`AudioDrive::Running`], mixes, soft-limits if SFX voices contributed, and ramps gain
    /// smoothly toward the target by at most [`FADE_STEP`] a sample.
    pub fn advance_frame_with(&mut self, drive: AudioDrive, voices: SfxVoices) {
        let mut sfx_active = false;
        let target_gain = match drive {
            AudioDrive::Running { target_gain } => {
                if let Some(frame_sounds) = self.pending_frames.pop_front() {
                    for id in frame_sounds.into_iter().flatten() {
                        self.route_request(id.byte(), voices);
                    }
                }
                for write in self.driver.tick() {
                    self.stream.apu_mut().write(write.addr, write.value);
                }
                sfx_active = self.sfx_pool.iter().any(|v| v.active);
                target_gain
            }
            AudioDrive::Frozen => 0.0,
        };
        let mut buffer = [0.0_f32; SAMPLES_PER_FRAME + NUDGE];
        let buffer = &mut buffer[..samples_for(self.queue.len())];
        self.stream.fill(buffer);
        if sfx_active {
            for voice in &mut self.sfx_pool {
                if voice.active {
                    voice.tick_and_mix(buffer);
                }
            }
            for sample in buffer.iter_mut() {
                *sample = soft_clip(*sample);
            }
        }
        for &sample in &*buffer {
            self.gain += (target_gain - self.gain).clamp(-FADE_STEP, FADE_STEP);
            self.queue.push(sample * self.gain);
        }
    }
}

/// Whether [`QueueSource`] is waiting for [`QUEUE_TARGET`] samples or draining them.
#[derive(Debug, Clone, Copy)]
enum QueueState {
    /// Waiting for the queue to fill while holding the last level (`0.0` at startup).
    Buffering { hold: f32 },
    /// Draining queued samples, remembering the last popped level on underrun.
    Draining { last: f32 },
}

/// A Bevy audio source that drains [`SampleQueue`] once it holds [`QUEUE_TARGET`].
pub(crate) struct QueueSource {
    queue: SampleQueue,
    state: QueueState,
}

impl Iterator for QueueSource {
    type Item = Sample;

    /// The last sample while the queue fills (from empty, or after running dry), so a
    /// gap is a held level and never a step; `None` would end the source for good.
    fn next(&mut self) -> Option<Self::Item> {
        let sample = match self.state {
            QueueState::Buffering { hold } => {
                if self.queue.len() >= QUEUE_TARGET
                    && let Some(sample) = self.queue.pop()
                {
                    self.state = QueueState::Draining { last: sample };
                    sample
                } else {
                    hold
                }
            }
            QueueState::Draining { last } => {
                if let Some(sample) = self.queue.pop() {
                    self.state = QueueState::Draining { last: sample };
                    sample
                } else {
                    self.state = QueueState::Buffering { hold: last };
                    last
                }
            }
        };
        Some(sample)
    }
}

impl Source for QueueSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(1).expect("one channel is non-zero")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        None // endless: the source follows the queue
    }
}

/// The playable asset: a handle to the shared queue.
#[derive(Asset, TypePath)]
pub(crate) struct MusicSource {
    queue: SampleQueue,
}

impl Decodable for MusicSource {
    type Decoder = QueueSource;

    fn decoder(&self) -> Self::Decoder {
        QueueSource {
            queue: self.queue.clone(),
            state: QueueState::Buffering { hold: 0.0 },
        }
    }
}

/// Audio plugin: keeps the synth on the game clock and plays its output.
#[derive(Default)]
pub(crate) struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        // `add_audio_source` schedules the system that plays `AudioPlayer<MusicSource>`;
        // `init_asset` alone makes the asset but nothing ever plays it.
        app.add_audio_source::<MusicSource>()
            .init_resource::<SampleQueue>()
            .add_systems(Startup, start_music)
            .add_systems(
                Update,
                advance_music
                    .in_set(crate::pacing::Step::Music)
                    .run_if(crate::pacing::due),
            );
    }
}

/// Hands the queue to Bevy's audio engine. Without a device this simply logs a
/// warning, as the engine already does, and the game carries on silently.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn start_music(
    mut commands: Commands<'_, '_>,
    queue: Res<'_, SampleQueue>,
    mut sources: ResMut<'_, Assets<MusicSource>>,
    rom: Res<'_, crate::RomData>,
) {
    let handle = sources.add(MusicSource {
        queue: queue.clone(),
    });
    commands.insert_resource(Music::new(queue.clone(), &rom.0.prg));
    commands.spawn(AudioPlayer::<MusicSource>(handle));
}

/// Advances the synth one frame per due simulation tick.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn advance_music(
    mut music: ResMut<'_, Music>,
    clock: Res<'_, crate::pacing::Clock>,
    settings: Res<'_, crate::settings::Settings>,
    menu: Res<'_, crate::menu::Menu>,
) {
    let drive = if menu.is_open() {
        AudioDrive::Frozen
    } else {
        AudioDrive::Running {
            target_gain: gain(settings.volume),
        }
    };
    for _ in 0..clock.ticks_due().max(1) {
        music.advance_frame_with(drive, settings.sfx_voices);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post_update_systems(with_audio: bool) -> usize {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        if with_audio {
            app.add_plugins(AudioPlugin);
        }
        app.world_mut()
            .schedule_scope(PostUpdate, |world, schedule| {
                schedule.initialize(world).expect("PostUpdate initializes");
                schedule.systems().expect("systems").count()
            })
    }

    /// Bevy plays an `AudioPlayer<T>` only for a `T` registered with `add_audio_source`:
    /// the asset's own `PostUpdate` system plus play and cleanup (`init_asset` alone adds 1).
    #[test]
    fn bevy_audio_plays_the_music_source() {
        assert_eq!(post_update_systems(true) - post_update_systems(false), 3);
    }

    /// The DMC plays the game's sample bank, not the zeros an empty sample ROM reads.
    #[test]
    fn the_dmc_reads_the_games_samples() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let dmc_level_after = |mut apu: rotj_apu::apu::Apu| {
            for (addr, value) in [
                (rotj_apu::dmc::LEVEL, 0x40),
                (rotj_apu::dmc::ADDRESS, 0x94),
                (rotj_apu::dmc::LENGTH, 0x3F),
                (rotj_apu::dmc::CTRL, 0x0F),
                (rotj_apu::STATUS, rotj_apu::status::DMC),
            ] {
                apu.write(addr, value);
            }
            (0..20_000)
                .map(|_| {
                    apu.tick();
                    apu.channel_levels()[4]
                })
                .max()
                .expect("ticks")
        };
        assert!(
            dmc_level_after(apu_with_samples(&rom.prg))
                > dmc_level_after(rotj_apu::apu::Apu::new())
        );
    }

    #[test]
    fn queue_round_trips_and_bounds_itself() {
        let queue = SampleQueue::default();
        assert_eq!(queue.len(), 0);
        queue.push(0.25);
        queue.push(-0.5);
        assert_eq!(queue.pop(), Some(0.25));
        assert_eq!(queue.pop(), Some(-0.5));
        assert_eq!(queue.pop(), None);
        for _ in 0..QUEUE_CAPACITY * 2 {
            queue.push(1.0);
        }
        assert_eq!(queue.len(), QUEUE_CAPACITY, "the queue is bounded");
    }

    #[test]
    fn decoder_waits_for_the_queue_then_drains_it() {
        let queue = SampleQueue::default();
        let source = MusicSource {
            queue: queue.clone(),
        };
        let mut decoder = source.decoder();
        queue.push(0.5);
        assert_eq!(decoder.next(), Some(0.0), "silent until the queue fills");
        assert_eq!(queue.len(), 1, "and nothing taken meanwhile");
        for _ in 1..QUEUE_TARGET {
            queue.push(0.25);
        }
        assert_eq!(decoder.next(), Some(0.5));
        assert_eq!(decoder.channels().get(), 1);
        assert_eq!(decoder.sample_rate().get(), SAMPLE_RATE);
    }

    #[test]
    fn an_underrun_holds_the_last_sample_and_refills() {
        let queue = SampleQueue::default();
        let mut decoder = MusicSource {
            queue: queue.clone(),
        }
        .decoder();
        for _ in 0..QUEUE_TARGET {
            queue.push(0.25);
        }
        for _ in 0..QUEUE_TARGET {
            assert_eq!(decoder.next(), Some(0.25));
        }
        assert_eq!(
            decoder.next(),
            Some(0.25),
            "starved: the last sample, never a step"
        );
        queue.push(0.5);
        assert_eq!(decoder.next(), Some(0.25), "refilling: still held");
    }

    #[test]
    fn the_frame_size_follows_the_queue() {
        assert_eq!(samples_for(0), SAMPLES_PER_FRAME + NUDGE);
        assert_eq!(samples_for(QUEUE_TARGET), SAMPLES_PER_FRAME);
        assert_eq!(samples_for(QUEUE_TARGET * 2), SAMPLES_PER_FRAME - NUDGE);
    }

    /// The ported driver, ticked on the game's own clock, produces real,
    /// non-silent audio: the game's own music, from the ROM's song data rather
    /// than a capture of it.
    #[test]
    fn music_advances_and_produces_sound() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let queue = SampleQueue::default();
        let mut music = Music::new(queue.clone(), &rom.prg);
        music.request(SoundId::from_byte(0x04).expect("a song"));
        for _ in 0..600 {
            music.advance_frame(AudioDrive::Running { target_gain: 1.0 });
        }
        // 600 frames of 44.1 kHz audio, bounded by the queue's capacity.
        assert_eq!(queue.len(), QUEUE_CAPACITY);
        let peak = drain_peak(&queue);
        assert!(peak > 0.05, "the music is audible (peak {peak})");
        assert!(peak <= 1.0, "and stays in range");
    }

    /// One frame of `music`, its samples appended to `samples`.
    fn frame(music: &mut Music, samples: &mut Vec<f32>, drive: AudioDrive) {
        music.advance_frame(drive);
        while let Some(sample) = music.queue.pop() {
            samples.push(sample);
        }
    }

    /// The title song at full gain for 60 frames, then `frames` with the menu open and
    /// as many closed again: every sample pushed, where the open began, the driver then,
    /// and the synth after.
    fn menu_round_trip(frames: usize) -> Option<(Vec<f32>, usize, rotj_music::Music, Music)> {
        let rom = rotj_rom::for_test()?;
        let mut music = Music::new(SampleQueue::default(), &rom.prg);
        music.request(SoundId::from_byte(0x01).expect("a song"));
        let mut samples = Vec::new();
        for _ in 0..60 {
            frame(
                &mut music,
                &mut samples,
                AudioDrive::Running { target_gain: 1.0 },
            );
        }
        let (opened, driver) = (samples.len(), music.driver.clone());
        for _ in 0..frames {
            frame(&mut music, &mut samples, AudioDrive::Frozen);
        }
        let closed = samples.len();
        for _ in 0..frames {
            frame(
                &mut music,
                &mut samples,
                AudioDrive::Running { target_gain: 1.0 },
            );
        }
        assert!(samples[opened + 441..closed].iter().all(|&s| s == 0.0));
        Some((samples, opened, driver, music))
    }

    #[test]
    fn the_menu_fades_out_and_back_in_without_a_step() {
        let Some((samples, opened, _, _)) = menu_round_trip(10) else {
            return;
        };
        let step = |pair: &[f32]| (pair[1] - pair[0]).abs();
        let music = samples[441..opened]
            .windows(2)
            .map(step)
            .fold(0.0, f32::max);
        let worst = samples[opened..].windows(2).map(step).fold(0.0, f32::max);
        assert!(music > 0.0, "the song plays before the menu opens");
        assert!(
            worst <= music + FADE_STEP,
            "a step of {worst}, the music's {music}"
        );
    }

    #[test]
    fn the_driver_does_not_run_while_open() {
        let Some((_, _, mut at_open, mut music)) = menu_round_trip(10) else {
            return;
        };
        for _ in 0..10 {
            at_open.tick();
        }
        for _ in 0..30 {
            assert_eq!(music.driver.tick(), at_open.tick());
        }
    }

    #[test]
    fn frame_zero_request_is_not_wiped_by_init() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let queue = SampleQueue::default();
        let mut music = Music::new(queue.clone(), &rom.prg);
        // Queuing a song immediately after construction (before any advance_frame) must stick.
        music.request(SoundId::from_byte(0x01).expect("title song"));
        for _ in 0..120 {
            music.advance_frame(AudioDrive::Running { target_gain: 1.0 });
        }
        assert!(drain_peak(&queue) > 0.05, "frame-0 request played");
    }

    #[test]
    fn gain_is_squared() {
        assert!((gain(100) - 1.0).abs() < f32::EPSILON);
        assert!((gain(50) - 0.25).abs() < f32::EPSILON);
        assert!(gain(0).abs() < f32::EPSILON);
    }

    #[test]
    fn soft_clip_is_identity_below_knee_and_bounded_under_overload() {
        for x in [-0.85_f32, -0.5, 0.0, 0.5, 0.85] {
            assert!((soft_clip(x) - x).abs() < f32::EPSILON, "x={x}");
        }
        for x in [-16.0_f32, -2.0, 2.0, 16.0] {
            let y = soft_clip(x);
            assert!(
                y.is_finite() && y.abs() <= 1.0 && y.abs() > 0.85,
                "x={x} y={y}"
            );
        }
    }

    #[test]
    fn unlimited_sfx_never_steals_music_channels_and_reclaims_voices() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let mut music_only = Music::new(SampleQueue::default(), &rom.prg);
        let mut unlimited = Music::new(SampleQueue::default(), &rom.prg);
        let song = SoundId::from_byte(0x04).expect("stage 1-1 song");
        let jump = SoundId::from_byte(rotj_music::JUMP_SFX).expect("jump sfx");
        music_only.request_frame([Some(song), None, None, None]);
        unlimited.request_frame([Some(song), None, None, None]);
        for frame in 0..80 {
            if frame == 10 || frame == 14 || frame == 18 {
                unlimited.request_frame([Some(jump), None, None, None]);
            }
            music_only.advance_frame_with(
                AudioDrive::Running { target_gain: 1.0 },
                SfxVoices::Unlimited,
            );
            unlimited.advance_frame_with(
                AudioDrive::Running { target_gain: 1.0 },
                SfxVoices::Unlimited,
            );
            // Primary music driver's register stream must remain 100% identical to music-only!
            assert_eq!(
                unlimited.driver.clone().tick(),
                music_only.driver.clone().tick(),
                "frame {frame}: SFX must not alter primary music driver"
            );
        }
        assert!(
            unlimited.sfx_pool.len() >= 2,
            "overlapping SFX allocated distinct voices"
        );
        assert!(
            unlimited.sfx_pool.iter().all(|v| !v.active),
            "finished SFX voices auto-reclaimed"
        );
    }

    #[test]
    fn unlimited_sfx_pool_bounds_worst_case_burst_and_stops_on_stop_all() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let queue = SampleQueue::default();
        let mut music = Music::new(queue.clone(), &rom.prg);
        let jump = SoundId::from_byte(rotj_music::JUMP_SFX).expect("jump sfx");
        let stop = SoundId::from_byte(0xFF).expect("stop all");
        // Trigger 20 rapid SFX (> MAX_SFX_VOICES=16) to exercise voice stealing & soft limiting.
        for _ in 0..5 {
            music.request_frame([Some(jump), Some(jump), Some(jump), Some(jump)]);
            music.advance_frame_with(
                AudioDrive::Running { target_gain: 1.0 },
                SfxVoices::Unlimited,
            );
        }
        assert_eq!(music.sfx_pool.len(), MAX_SFX_VOICES);
        let peak = drain_peak(&queue);
        assert!(peak > 0.05 && peak <= 1.0, "bounded burst peak {peak}");

        music.request_frame([Some(stop), None, None, None]);
        music.advance_frame_with(
            AudioDrive::Running { target_gain: 1.0 },
            SfxVoices::Unlimited,
        );
        assert!(
            music.sfx_pool.iter().all(|v| !v.active),
            "StopAll silences the SFX pool"
        );
    }

    /// Drains a queue, returning the loudest sample seen.
    fn drain_peak(queue: &SampleQueue) -> f32 {
        let mut peak = 0.0_f32;
        while let Some(sample) = queue.pop() {
            peak = peak.max(sample.abs());
        }
        peak
    }
}
