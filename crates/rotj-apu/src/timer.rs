//! Internal divider used by the channel cores.
//!
//! Clocking is tick-counting with the semantics of the hardware timer: the
//! counter runs 0..=`period` inclusively, so an expiry every
//! `period + 1` ticks ( nesdev, "APU Pulse" — the pulse timer counts
//! t, t-1, …, 0 and clocks the sequencer when it wraps).
#[derive(Debug, Clone, Copy)]
pub(super) struct Timer {
    counter: u16,
    period: u16,
}

impl Timer {
    /// A timer counting `period + 1` ticks per expiry.
    pub(super) const fn new(period: u16) -> Self {
        Self { counter: 0, period }
    }

    pub(super) const fn set_period(&mut self, period: u16) {
        self.period = period;
    }

    /// Advances one tick; returns `true` when the divider expires.
    ///
    /// Like Tetanes's [`Timer`], a freshly reset timer (counter 0) expires on
    /// its first tick, matching the hardware's immediate sequencer restart on
    /// `$4003`-side effects.
    ///
    /// [`Timer`]: https://github.com/lukexor/tetanes
    pub(super) fn tick(&mut self) -> bool {
        if self.counter == 0 {
            self.counter = self.period;
            true
        } else {
            self.counter -= 1;
            false
        }
    }

    /// Divider period, in ticks per expiry minus one.
    pub(super) const fn period(self) -> u16 {
        self.period
    }

    /// Current count within the divider period.
    #[cfg(test)]
    pub(super) const fn counter(self) -> u16 {
        self.counter
    }
}
