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
//! (`docs/MUSIC.md`, `crates/rotj-music/tests/acceptance.rs`).
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
pub fn gain(volume: u8) -> f32 {
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
        prg[0x1E000..0x20000].to_vec(),
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
pub struct SampleQueue(Arc<Mutex<VecDeque<f32>>>);

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

/// The synth on the game's clock: ticks the ported driver each frame, applies
/// the register writes it returns, and fills the queue.
#[derive(Resource)]
pub struct Music {
    stream: ApuStream,
    driver: rotj_music::Music,
    started: bool,
    queue: SampleQueue,
    /// The gain applied to the last sample, ramping toward the asked one.
    gain: f32,
}

impl Music {
    /// Builds the synth and loads the driver.
    #[must_use]
    pub fn new(queue: SampleQueue, prg: &[u8]) -> Self {
        Self {
            stream: ApuStream::new(apu_with_samples(prg), SAMPLE_RATE),
            driver: rotj_music::Music::from_prg(prg),
            started: false,
            queue,
            gain: 0.0,
        }
    }

    /// Queues a request the game made (`$40-$43`).
    pub fn request(&mut self, id: u8) {
        self.driver.request(id);
    }

    /// The driver's power-on init (bank 10 `$8000`, from `$D394`), once.
    pub fn start(&mut self) {
        if !self.started {
            for write in self.driver.init() {
                self.stream.apu_mut().write(write.addr, write.value);
            }
            self.started = true;
        }
    }

    /// Advances one game frame: tick the driver (unless the game is frozen) and apply its
    /// writes, then render about a frame's worth of samples ([`samples_for`]), the gain
    /// moving toward `gain` by at most [`FADE_STEP`] a sample.
    pub fn advance_frame(&mut self, run_driver: bool, gain: f32) {
        if run_driver {
            for write in self.driver.tick() {
                self.stream.apu_mut().write(write.addr, write.value);
            }
        }
        let mut buffer = [0.0_f32; SAMPLES_PER_FRAME + NUDGE];
        let buffer = &mut buffer[..samples_for(self.queue.len())];
        self.stream.fill(buffer);
        for &sample in &*buffer {
            self.gain += (gain - self.gain).clamp(-FADE_STEP, FADE_STEP);
            self.queue.push(sample * self.gain);
        }
    }
}

/// A Bevy audio source that drains [`SampleQueue`] once it holds [`QUEUE_TARGET`].
pub struct QueueSource {
    queue: SampleQueue,
    playing: bool,
    last: f32,
}

impl Iterator for QueueSource {
    type Item = Sample;

    /// The last sample while the queue fills (from empty, or after running dry), so a
    /// gap is a held level and never a step; `None` would end the source for good.
    fn next(&mut self) -> Option<Self::Item> {
        self.playing = self.playing || self.queue.len() >= QUEUE_TARGET;
        if self.playing {
            match self.queue.pop() {
                Some(sample) => self.last = sample,
                None => self.playing = false,
            }
        }
        Some(self.last)
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
pub struct MusicSource {
    queue: SampleQueue,
}

impl Decodable for MusicSource {
    type Decoder = QueueSource;

    fn decoder(&self) -> Self::Decoder {
        QueueSource {
            queue: self.queue.clone(),
            playing: false,
            last: 0.0,
        }
    }
}

/// Audio plugin: keeps the synth on the game clock and plays its output.
#[derive(Default)]
pub struct AudioPlugin;

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

/// Inits the driver on the first tick, then advances the synth one frame.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn advance_music(
    mut music: ResMut<'_, Music>,
    settings: Res<'_, crate::settings::Settings>,
    menu: Res<'_, crate::menu::Menu>,
) {
    music.start();
    let gain = if menu.open {
        0.0
    } else {
        gain(settings.volume)
    };
    music.advance_frame(!menu.open, gain);
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
        music.start();
        music.request(0x04);
        for _ in 0..600 {
            music.advance_frame(true, 1.0);
        }
        assert!(music.started, "the driver was asked for a song");
        // 600 frames of 44.1 kHz audio, bounded by the queue's capacity.
        assert_eq!(queue.len(), QUEUE_CAPACITY);
        let peak = drain_peak(&queue);
        assert!(peak > 0.05, "the music is audible (peak {peak})");
        assert!(peak <= 1.0, "and stays in range");
    }

    /// One frame of `music`, its samples appended to `samples`.
    fn frame(music: &mut Music, samples: &mut Vec<f32>, run: bool, gain: f32) {
        music.advance_frame(run, gain);
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
        music.start();
        music.request(0x01);
        let mut samples = Vec::new();
        for _ in 0..60 {
            frame(&mut music, &mut samples, true, 1.0);
        }
        let (opened, driver) = (samples.len(), music.driver.clone());
        for _ in 0..frames {
            frame(&mut music, &mut samples, false, 0.0);
        }
        let closed = samples.len();
        for _ in 0..frames {
            frame(&mut music, &mut samples, true, 1.0);
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
    fn gain_is_squared() {
        assert!((gain(100) - 1.0).abs() < f32::EPSILON);
        assert!((gain(50) - 0.25).abs() < f32::EPSILON);
        assert!(gain(0).abs() < f32::EPSILON);
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
