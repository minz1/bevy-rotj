//! NES DAC mixing: the two resistor networks that sum the channel levels into
//! one analog output, per nesdev's "APU Mixer" page (the formulas emulators use
//! to reproduce the console's output level).
//!
//! Levels are the raw channel outputs: `0..=15` for pulse/triangle/noise and
//! `0..=127` for DMC.

/// Analog level from the two pulse channels.
#[must_use]
pub fn pulse_out(pulse1: u8, pulse2: u8) -> f32 {
    let sum = f32::from(pulse1) + f32::from(pulse2);
    if sum == 0.0 {
        0.0
    } else {
        95.88 / (8128.0 / sum + 100.0)
    }
}

/// Analog level from the triangle, noise and DMC channels.
#[must_use]
pub fn tnd_out(triangle: u8, noise: u8, dmc: u8) -> f32 {
    let sum = f32::from(triangle) / 8227.0 + f32::from(noise) / 12241.0 + f32::from(dmc) / 22638.0;
    if sum == 0.0 {
        0.0
    } else {
        159.79 / (1.0 / sum + 100.0)
    }
}

/// Analog output level, `0.0..=1.0` at the hardware's calibration.
#[must_use]
pub fn mix(pulse1: u8, pulse2: u8, triangle: u8, noise: u8, dmc: u8) -> f32 {
    pulse_out(pulse1, pulse2) + tnd_out(triangle, noise, dmc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formulas are calibrated so that everything at full scale lands just
    /// at (or a hair over) unity.
    #[test]
    fn full_scale_is_unity() {
        assert!((mix(15, 15, 15, 15, 127) - 1.0).abs() < 0.001);
    }

    #[test]
    fn silence_is_silence() {
        assert_eq!(mix(0, 0, 0, 0, 0), 0.0);
        assert_eq!(pulse_out(0, 0), 0.0);
        assert_eq!(tnd_out(0, 0, 0), 0.0);
    }

    #[test]
    fn published_midpoints_match() {
        // 95.88 / (8128 / 30 + 100) and the triangle-only case.
        assert!((pulse_out(15, 15) - 0.2585).abs() < 0.001);
        assert!((tnd_out(15, 0, 0) - 0.2464).abs() < 0.001);
    }

    #[test]
    fn output_rises_with_level() {
        assert!(mix(8, 0, 0, 0, 0) > mix(1, 0, 0, 0, 0));
        assert!(mix(0, 0, 0, 8, 0) > mix(0, 0, 0, 1, 0));
    }
}
