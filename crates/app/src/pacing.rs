//! One picture per 60 Hz tick. winit waits for each tick on its own fixed-rate schedule
//! (`UpdateMode::Reactive`, a high-resolution timer on Windows) and hands over the OS's
//! input events just before the update, so the tick reads a fresh pad.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

/// The tick's length.
pub const PERIOD: Duration = Duration::from_nanos(16_666_667);

/// The frame's work, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// The game's tick.
    Game,
    /// The music driver's frame and the synth.
    Music,
    /// The picture.
    Picture,
}

/// Whether a frame starting at `now` runs a tick, the last having run at `last`: an
/// update half a period or less after it (a redraw request) does not, so the game never
/// runs two ticks in one period.
#[must_use]
pub fn tick_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) > PERIOD / 2)
}

/// When the last tick ran, and whether this frame runs one.
#[derive(Resource, Default)]
pub struct Clock {
    last: Option<Instant>,
    due: bool,
    log: Option<FrameLog>,
}

/// The frame runs the game's tick and the music's frame.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub fn due(clock: Res<'_, Clock>) -> bool {
    clock.due
}

/// Tick intervals, summarised every [`FrameLog::EVERY`] ticks (`ROTJ_FRAME_LOG=1`).
#[derive(Default)]
struct FrameLog {
    count: u32,
    sum: f64,
    squares: f64,
    worst: f64,
}

impl FrameLog {
    const EVERY: u32 = 300;

    fn record(&mut self, ms: f64) {
        self.count += 1;
        self.sum += ms;
        self.squares += ms * ms;
        self.worst = self.worst.max(ms);
        if self.count < Self::EVERY {
            return;
        }
        let count = f64::from(self.count);
        let mean = self.sum / count;
        let sd = (self.squares / count - mean * mean).max(0.0).sqrt();
        info!(
            "frame interval over {count} frames: mean {mean:.3} ms, sd {sd:.3} ms, worst {:.3} ms",
            self.worst
        );
        *self = Self::default();
    }
}

fn start_frame(mut clock: ResMut<'_, Clock>) {
    let now = Instant::now();
    clock.due = tick_due(clock.last, now);
    if !clock.due {
        return;
    }
    if let (Some(last), Some(log)) = (clock.last, &mut clock.log) {
        log.record(now.duration_since(last).as_secs_f64() * 1000.0);
    }
    clock.last = Some(now);
}

/// Paces the app's frames to the tick.
pub struct PacingPlugin;

impl Plugin for PacingPlugin {
    fn build(&self, app: &mut App) {
        let mode = UpdateMode::Reactive {
            wait: PERIOD,
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        };
        let log = std::env::var("ROTJ_FRAME_LOG")
            .is_ok_and(|value| value == "1")
            .then(FrameLog::default);
        app.insert_resource(WinitSettings {
            focused_mode: mode,
            unfocused_mode: mode,
        })
        .insert_resource(Clock {
            log,
            ..Clock::default()
        })
        .add_systems(First, start_frame)
        .configure_sets(Update, (Step::Game, Step::Music, Step::Picture).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_first_frame_ticks() {
        assert!(tick_due(None, Instant::now()));
    }

    #[test]
    fn a_frame_a_period_on_ticks_even_a_little_early() {
        let last = Instant::now();
        assert!(tick_due(Some(last), last + PERIOD));
        assert!(tick_due(Some(last), last + 15 * MS));
    }

    #[test]
    fn a_redraw_soon_after_a_tick_does_not_tick() {
        let last = Instant::now();
        assert!(!tick_due(Some(last), last + MS));
        assert!(!tick_due(Some(last), last + PERIOD / 2));
    }
}
