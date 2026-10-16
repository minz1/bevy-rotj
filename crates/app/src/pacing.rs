//! One picture per 60 Hz tick: each frame waits for its deadline, then reads the pad,
//! runs the tick and draws, so a VRR display shows every tick for the same 1/60 s.

use std::time::{Duration, Instant};

use bevy::prelude::*;

/// The tick's length.
pub const PERIOD: Duration = Duration::from_nanos(16_666_667);

/// How much of a wait is slept rather than spun: the OS timer's slack.
const SPIN: Duration = Duration::from_millis(1);

/// The frame's work after the wait, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// The game's tick.
    Game,
    /// The music driver's frame and the synth.
    Music,
    /// The picture.
    Picture,
}

/// The deadline after `deadline`, once `now` has reached it: a period on, or a period from
/// `now` after a stall of more than a period, so a stall never becomes a burst of ticks.
#[must_use]
pub fn next_deadline(deadline: Instant, now: Instant, period: Duration) -> Instant {
    if now > deadline + period {
        now + period
    } else {
        deadline + period
    }
}

/// Sleeps to [`SPIN`] short of `deadline`, then spins to it.
fn wait_until(deadline: Instant) {
    if let Some(sleep) = deadline
        .checked_duration_since(Instant::now())
        .and_then(|left| left.checked_sub(SPIN))
    {
        std::thread::sleep(sleep);
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

#[derive(Resource)]
struct Limiter {
    deadline: Instant,
    log: Option<FrameLog>,
}

/// Frame intervals, summarised every [`FrameLog::EVERY`] frames (`ROTJ_FRAME_LOG=1`).
struct FrameLog {
    last: Instant,
    intervals: Vec<f64>,
}

impl FrameLog {
    const EVERY: usize = 300;

    fn record(&mut self, now: Instant) {
        self.intervals
            .push((now - self.last).as_secs_f64() * 1000.0);
        self.last = now;
        if self.intervals.len() < Self::EVERY {
            return;
        }
        let count = f64::from(u32::try_from(self.intervals.len()).expect("300"));
        let mean = self.intervals.iter().sum::<f64>() / count;
        let variance = self
            .intervals
            .iter()
            .map(|ms| (ms - mean).powi(2))
            .sum::<f64>()
            / count;
        let worst = self.intervals.iter().copied().fold(0.0, f64::max);
        info!(
            "frame interval over {count} frames: mean {mean:.3} ms, sd {:.3} ms, worst {worst:.3} ms",
            variance.sqrt()
        );
        self.intervals.clear();
    }
}

fn wait(mut limiter: ResMut<'_, Limiter>) {
    wait_until(limiter.deadline);
    let now = Instant::now();
    limiter.deadline = next_deadline(limiter.deadline, now, PERIOD);
    if let Some(log) = &mut limiter.log {
        log.record(now);
    }
}

/// Paces the app's frames to the tick.
pub struct PacingPlugin;

impl Plugin for PacingPlugin {
    fn build(&self, app: &mut App) {
        let now = Instant::now();
        let log = std::env::var("ROTJ_FRAME_LOG")
            .is_ok_and(|value| value == "1")
            .then(|| FrameLog {
                last: now,
                intervals: Vec::with_capacity(FrameLog::EVERY),
            });
        app.insert_resource(Limiter { deadline: now, log })
            .add_systems(First, wait)
            .configure_sets(Update, (Step::Game, Step::Music, Step::Picture).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn on_time_advances_by_a_period() {
        let t = Instant::now() + Duration::from_secs(1);
        let early = t.checked_sub(3 * MS).expect("after the clock's start");
        assert_eq!(next_deadline(t, early, PERIOD), t + PERIOD);
    }

    #[test]
    fn a_little_late_keeps_the_rhythm() {
        let t = Instant::now();
        assert_eq!(next_deadline(t, t + 5 * MS, PERIOD), t + PERIOD);
    }

    #[test]
    fn a_stall_resets_instead_of_bursting() {
        let t = Instant::now();
        let now = t + Duration::from_secs(3);
        assert_eq!(next_deadline(t, now, PERIOD), now + PERIOD);
    }

    #[test]
    #[ignore = "real time: run with --release --ignored"]
    fn real_time_frames_average_a_sixtieth() {
        let start = Instant::now();
        let mut deadline = start;
        let mut errors = Vec::new();
        for _ in 0..600 {
            wait_until(deadline);
            let now = Instant::now();
            errors.push((now - deadline).as_secs_f64() * 1000.0);
            deadline = next_deadline(deadline, now, PERIOD);
        }
        let mean = start.elapsed().as_secs_f64() / 600.0;
        errors.sort_by(f64::total_cmp);
        println!(
            "mean frame {:.4} ms, 99th percentile wake error {:.4} ms",
            mean * 1000.0,
            errors[594]
        );
        assert!(
            (mean / PERIOD.as_secs_f64() - 1.0).abs() < 0.01,
            "mean {mean}"
        );
        assert!(errors[594] < 0.5, "99th percentile {} ms", errors[594]);
    }
}
