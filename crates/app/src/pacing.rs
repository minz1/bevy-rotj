//! Frame pacing: 60 Hz reactive lockstep in Classic mode, or continuous high-refresh
//! fixed-timestep accumulation with interpolation alpha (`frame_rate` setting).

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::winit::{UpdateMode, WinitSettings};

use crate::settings::{FrameRate, Settings};

/// The 60 Hz simulation tick length.
pub(crate) const PERIOD: Duration = Duration::from_nanos(16_666_667);
/// Maximum ticks caught up in a single render frame (guards against spiral-of-death after OS stalls).
const MAX_CATCHUP_TICKS: u32 = 4;

/// The frame's work, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Step {
    /// The game's tick(s).
    Game,
    /// The music driver's frame(s) and the synth.
    Music,
    /// The picture.
    Picture,
}

/// Whether a Classic 60 Hz frame starting at `now` runs a tick, the last having run at `last`:
/// an update half a period or less after it (a redraw request) does not, so the game never
/// runs two ticks in one period.
#[must_use]
pub(crate) fn tick_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) > PERIOD / 2)
}

/// Pacing state for either Classic reactive 60 Hz or high-refresh fixed-timestep accumulation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TickState {
    /// No frame has run yet; the first frame always runs one tick.
    #[default]
    Initial,
    /// Classic 60 Hz mode: tracks the last tick instant.
    Classic { last: Instant },
    /// High-refresh mode: tracks the last frame instant and accumulated sub-tick time.
    Continuous { last_frame: Instant, acc: Duration },
}

/// Simulation clock: how many 60 Hz ticks are due this render frame, and the interpolation
/// fraction `alpha ∈ [0.0, 1.0]` between the previous and current tick.
#[derive(Resource)]
pub(crate) struct Clock {
    state: TickState,
    ticks_due: u32,
    alpha: f32,
    next_deadline: Option<Instant>,
    log: Option<FrameLog>,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            state: TickState::Initial,
            ticks_due: 0,
            alpha: 1.0,
            next_deadline: None,
            log: None,
        }
    }
}

impl Clock {
    /// Number of 60 Hz simulation ticks due on this render frame (`0..=4`).
    #[must_use]
    pub const fn ticks_due(&self) -> u32 {
        self.ticks_due
    }

    /// Interpolation fraction between the previous and current 60 Hz tick (`1.0` in Classic mode).
    #[must_use]
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Advances the clock to `now` for `frame_rate`, updating `ticks_due` and `alpha`.
    fn advance(&mut self, now: Instant, frame_rate: FrameRate) {
        if frame_rate.is_classic() {
            self.next_deadline = None;
            let last = match self.state {
                TickState::Classic { last } => Some(last),
                TickState::Continuous { last_frame, .. } => Some(last_frame),
                TickState::Initial => None,
            };
            self.alpha = 1.0;
            if !tick_due(last, now) {
                self.ticks_due = 0;
                if let Some(last) = last {
                    self.state = TickState::Classic { last };
                }
                return;
            }
            if let (Some(last), Some(log)) = (last, &mut self.log) {
                log.record(now.duration_since(last).as_secs_f64() * 1000.0);
            }
            self.state = TickState::Classic { last: now };
            self.ticks_due = 1;
            return;
        }

        match self.state {
            TickState::Initial => {
                self.state = TickState::Continuous {
                    last_frame: now,
                    acc: Duration::ZERO,
                };
                self.ticks_due = 1;
                self.alpha = 1.0;
            }
            TickState::Classic { last } => {
                let dt = now.duration_since(last).min(PERIOD * MAX_CATCHUP_TICKS);
                self.accumulate(now, dt);
            }
            TickState::Continuous { last_frame, acc } => {
                let dt = now
                    .duration_since(last_frame)
                    .min(PERIOD * MAX_CATCHUP_TICKS);
                self.accumulate(now, acc + dt);
            }
        }
    }

    fn accumulate(&mut self, now: Instant, mut acc: Duration) {
        let mut ticks = 0u32;
        while acc >= PERIOD && ticks < MAX_CATCHUP_TICKS {
            acc = acc.saturating_sub(PERIOD);
            ticks += 1;
        }
        if ticks == MAX_CATCHUP_TICKS && acc >= PERIOD {
            acc = Duration::ZERO;
        }
        if let Some(log) = &mut self.log
            && let TickState::Continuous { last_frame, .. } = self.state
        {
            log.record(now.duration_since(last_frame).as_secs_f64() * 1000.0);
        }
        self.state = TickState::Continuous {
            last_frame: now,
            acc,
        };
        self.ticks_due = ticks;
        self.alpha = (acc.as_secs_f32() / PERIOD.as_secs_f32()).clamp(0.0, 1.0);
    }
}

/// The frame runs at least one 60 Hz game and music tick.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
pub(crate) fn due(clock: Res<'_, Clock>) -> bool {
    clock.ticks_due > 0
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

/// Desired [`UpdateMode`] for `frame_rate`.
fn update_mode(frame_rate: FrameRate) -> UpdateMode {
    if frame_rate.is_classic() {
        UpdateMode::Reactive {
            wait: PERIOD,
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        }
    } else {
        UpdateMode::Continuous
    }
}

/// Enforces explicit high-refresh FPS caps (`FrameRate::Fps(n)` with `n > 60`) via a monotonic
/// deadline so `vsync: false` never busy-spins at thousands of FPS.
fn pace_explicit_fps(clock: &mut Clock, frame_rate: FrameRate) {
    let FrameRate::Fps(fps) = frame_rate else {
        clock.next_deadline = None;
        return;
    };
    if fps <= FrameRate::MIN_FPS {
        clock.next_deadline = None;
        return;
    }
    let target = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut now = Instant::now();
    if let Some(deadline) = clock.next_deadline {
        if now < deadline {
            let remaining = deadline.duration_since(now);
            if remaining > Duration::from_micros(1500) {
                std::thread::sleep(remaining.saturating_sub(Duration::from_micros(1000)));
            }
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
            now = Instant::now();
        }
        let next = deadline + target;
        clock.next_deadline = Some(if next < now { now + target } else { next });
    } else {
        clock.next_deadline = Some(now + target);
    }
}

fn start_frame(mut clock: ResMut<'_, Clock>, settings: Option<Res<'_, Settings>>) {
    let frame_rate = settings.map_or(FrameRate::Classic, |s| s.frame_rate);
    pace_explicit_fps(&mut clock, frame_rate);
    clock.advance(Instant::now(), frame_rate);
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy system params, rust-clippy#8940"
)]
fn sync_winit_mode(
    settings: Res<'_, Settings>,
    mut winit: ResMut<'_, WinitSettings>,
    mut last: Local<'_, Option<FrameRate>>,
) {
    if *last == Some(settings.frame_rate) {
        return;
    }
    *last = Some(settings.frame_rate);
    let mode = update_mode(settings.frame_rate);
    winit.focused_mode = mode;
    winit.unfocused_mode = mode;
}

/// Paces the app's frames to the tick or high-refresh target.
pub(crate) struct PacingPlugin;

impl Plugin for PacingPlugin {
    fn build(&self, app: &mut App) {
        let mode = update_mode(FrameRate::Classic);
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
        .add_systems(First, (sync_winit_mode, start_frame).chain())
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

    #[test]
    fn continuous_clock_accumulates_sub_ticks_and_computes_alpha() {
        let mut clock = Clock::default();
        let t0 = Instant::now();
        clock.advance(t0, FrameRate::Display);
        assert_eq!(clock.ticks_due(), 1);
        assert!((clock.alpha() - 1.0).abs() < f32::EPSILON);

        // Half a 60 Hz tick later (~120 Hz): 0 sim ticks, alpha ≈ 0.5.
        let t1 = t0 + PERIOD / 2;
        clock.advance(t1, FrameRate::Display);
        assert_eq!(clock.ticks_due(), 0);
        assert!(
            (clock.alpha() - 0.5).abs() < 0.01,
            "alpha {}",
            clock.alpha()
        );

        // Another half tick later: 1 sim tick, alpha ≈ 0.0.
        let t2 = t0 + PERIOD;
        clock.advance(t2, FrameRate::Display);
        assert_eq!(clock.ticks_due(), 1);
        assert!(clock.alpha() < 0.01, "alpha {}", clock.alpha());
    }

    #[test]
    fn continuous_clock_clamps_spiral_of_death() {
        let mut clock = Clock::default();
        let t0 = Instant::now();
        clock.advance(t0, FrameRate::Fps(170));
        // Simulate a 1-second OS stall: must clamp to MAX_CATCHUP_TICKS.
        clock.advance(t0 + Duration::from_secs(1), FrameRate::Fps(170));
        assert_eq!(clock.ticks_due(), MAX_CATCHUP_TICKS);
    }

    #[test]
    fn classic_mode_always_reports_alpha_one() {
        let mut clock = Clock::default();
        let t0 = Instant::now();
        clock.advance(t0, FrameRate::Classic);
        assert_eq!(clock.ticks_due(), 1);
        assert!((clock.alpha() - 1.0).abs() < f32::EPSILON);
        clock.advance(t0 + MS, FrameRate::Classic);
        assert_eq!(clock.ticks_due(), 0);
        assert!((clock.alpha() - 1.0).abs() < f32::EPSILON);
    }
}
