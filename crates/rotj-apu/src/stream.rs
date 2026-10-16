//! Sample-rate conversion: the APU runs on 2A03 ticks (~1.79 MHz on NTSC), but
//! audio devices want 44.1/48 kHz, so this resamples the APU's output for the
//! audio callback.
//!
//! Output is the mixer's level directly: [`crate::mixer`] yields `0.0` at
//! silence and `0.0..=1.0` while channels play, which is already the shape
//! `rodio`/`cpal` want (`f32` in `-1.0..=1.0`), so a silent APU is a silent
//! stream. (The hardware's own analog output has a small DC offset; the port
//! does not reproduce that.)

use crate::apu::Apu;

/// APU tick rate on NTSC (`NTSC_MASTER_CLOCK_RATE / 12 / 2`).
pub const NTSC_APU_HZ: f64 = 1_789_773.0 / 2.0;

/// An [`Apu`] resampled to a host sample rate.
#[derive(Debug, Clone)]
pub struct ApuStream {
    apu: Apu,
    /// APU ticks per output sample.
    ticks_per_sample: f64,
    /// Fractional ticks carried between samples.
    carry: f64,
    sample_rate: u32,
    /// APU ticks clocked so far (diagnostics, and the tests' timing check).
    ticks: u64,
}

impl ApuStream {
    /// Stream `apu` at `sample_rate` Hz.
    #[must_use]
    pub fn new(apu: Apu, sample_rate: u32) -> Self {
        assert!(sample_rate > 0, "sample rate must be positive");
        Self {
            apu,
            ticks_per_sample: NTSC_APU_HZ / f64::from(sample_rate),
            carry: 0.0,
            sample_rate,
            ticks: 0,
        }
    }

    /// The APU being streamed (write sound-driver registers through this).
    pub const fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// The host sample rate this stream was built for.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// APU ticks clocked since creation.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Next output sample, advancing the APU by the ticks that fall inside it.
    fn next_sample(&mut self) -> f32 {
        self.carry += self.ticks_per_sample;
        while self.carry >= 1.0 {
            self.apu.tick();
            self.ticks += 1;
            self.carry -= 1.0;
        }
        self.apu.output()
    }

    /// Fill `out` with the next samples (the audio-callback shape).
    pub fn fill(&mut self, out: &mut [f32]) {
        for sample in out {
            *sample = self.next_sample();
        }
    }
}

impl Iterator for ApuStream {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        Some(self.next_sample())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apu::Apu;

    #[test]
    fn ticks_advance_at_the_apu_rate() {
        let mut stream = ApuStream::new(Apu::new(), 44_100);
        for _ in 0..44_100 {
            let _ = stream.next();
        }
        let expected = NTSC_APU_HZ;
        let actual = f64::from(u32::try_from(stream.ticks()).expect("fits"));
        assert!(
            (actual - expected).abs() < expected * 0.001,
            "one second of samples should clock ~{expected} ticks, got {actual}"
        );
    }

    #[test]
    fn a_silent_apu_streams_zeros() {
        let mut stream = ApuStream::new(Apu::new(), 48_000);
        let mut buffer = [1.0f32; 1_024];
        stream.fill(&mut buffer);
        assert!(
            buffer.iter().all(|sample| *sample == 0.0),
            "silence stays silent"
        );
    }

    #[test]
    fn a_playing_channel_reaches_the_stream() {
        let mut apu = Apu::new();
        apu.write(0x4015, 0x01); // enable pulse 1
        apu.write(0x4000, 0xbf); // duty, halt, constant volume 15
        apu.write(0x4002, 0x40);
        apu.write(0x4003, 0x08);
        let mut stream = ApuStream::new(apu, 48_000);
        let mut peak = 0.0f32;
        for _ in 0..48_000 {
            peak = peak.max(stream.next().expect("stream never ends"));
        }
        assert!(peak > 0.0, "the channel should be audible");
        assert!(peak <= 1.0);
    }

    #[test]
    fn fill_matches_the_iterator() {
        let mut by_fill = ApuStream::new(Apu::new(), 44_100);
        let mut by_iter = ApuStream::new(Apu::new(), 44_100);
        let mut buffer = [0.0f32; 256];
        by_fill.fill(&mut buffer);
        for sample in buffer {
            assert_eq!(sample, by_iter.next().expect("stream never ends"));
        }
        assert_eq!(by_fill.ticks(), by_iter.ticks());
    }

    /// Rising crossings of the mean over one second of `stream`.
    fn hz(mut stream: ApuStream) -> usize {
        let samples: Vec<f32> = (0..44_100)
            .map(|_| stream.next().expect("endless"))
            .collect();
        let mean = samples.iter().sum::<f32>() / 44_100.0;
        samples
            .windows(2)
            .filter(|w| w[0] < mean && w[1] >= mean)
            .count()
    }

    /// Pulse 1 at period 253 is A440 (CPU / (16 * 254)).
    #[test]
    fn pulse_period_253_is_a440() {
        let mut stream = ApuStream::new(Apu::new(), 44_100);
        for (addr, value) in [
            (0x4015, 0x01),
            (0x4000, 0xBF),
            (0x4002, 253),
            (0x4003, 0x08),
        ] {
            stream.apu_mut().write(addr, value);
        }
        let hz = hz(stream);
        assert!((435..=445).contains(&hz), "{hz} Hz");
    }

    /// The triangle at period 126 is A440 (CPU / (32 * 127)).
    #[test]
    fn triangle_period_126_is_a440() {
        let mut stream = ApuStream::new(Apu::new(), 44_100);
        for (addr, value) in [
            (0x4015, 0x04),
            (0x4008, 0xFF),
            (0x400A, 126),
            (0x400B, 0x08),
        ] {
            stream.apu_mut().write(addr, value);
        }
        let hz = hz(stream);
        assert!((435..=445).contains(&hz), "{hz} Hz");
    }
}
