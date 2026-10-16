//! Frame counter and channels running together, per nesdev's "APU Frame
//! Counter" step table (NTSC). Events land at ceil(CPU_cycles / 2) APU
//! ticks per the tables 7457/14913/22371/29828/29829, measured with ±1 tick
//! tolerance for the CPU-cycle rounding.

use rotj_apu::frame_counter::{FrameCounter, FrameEvent};
use rotj_apu::pulse::{Pulse, PulseChannel};

/// A full 4-step sequence spans 14915 APU ticks: quarters at 3729 and
/// 11186, halves at 7457 and 14915, IRQ set near sequence end, and the
/// sequence repeats without drift.
#[test]
fn four_step_sequence_matches_the_step_table() {
    let mut frames = FrameCounter::new();
    let (quarters, halves) = drive(&mut frames, 32_000);

    // Sequence 1: the nesdev step offsets.
    assert_near(quarters[0], 3729, "quarter @7457 CPU cycles");
    assert_near(halves[0], 7457, "half @14913 CPU cycles");
    assert_near(quarters[1], 11186, "quarter @22371 CPU cycles");
    assert_near(halves[1], 14915, "half @29829 CPU cycles");
    // Sequence boundary = 29830 CPU cycles = 14915 APU ticks; sequence 2
    // must re-land exactly one sequence later.
    assert_near(quarters[2], 14915 + 3729, "seq2 quarter 1, no drift");
    assert_near(halves[2], 14915 + 7457, "seq2 half 1, no drift");
    assert_near(quarters[3], 14915 + 11186, "seq2 quarter 2, no drift");
    assert_near(halves[3], 29830, "seq2 half pair at the wrap tick");
    assert_eq!(
        halves[2] - halves[0],
        14915,
        "second pair offsets by a sequence"
    );
    assert_eq!(halves[3] - halves[1], 14915, "third pair likewise");

    assert_eq!(quarters.len(), 4, "exactly two quarter events per sequence");
    assert_eq!(halves.len(), 4, "exactly two half events per sequence");
    assert!(frames.irq_pending(), "mode 0 sets IRQ at step 4");
}

/// 5-step mode produces no IRQ, lands the `$4017` write's immediate
/// quarter+half frame within the settle delay, and sequences one
/// 18641-CPU-cycle block.
#[test]
fn five_step_mode_has_no_irq_and_immediate_frame() {
    let mut frames = FrameCounter::new();
    frames.write(0x80); // mode 1, IRQ not inhibited (but never set)
    let (quarters, halves) = drive(&mut frames, 20_000);

    assert!(halves[0] <= 3, "immediate half frame right after the write");
    assert_eq!(quarters.len(), 2, "one quarter pair per 5-step sequence");
    assert_eq!(
        halves.len(),
        3,
        "immediate frame + the two half clocks of the 5-step sequence"
    );
    assert_near(quarters[0], 3729, "5-step quarter at step 1");
    assert_near(quarters[1], 11186, "5-step quarter at step 3");
    assert_near(halves[1], 7457, "5-step half at step 2");
    assert_near(halves[2], 18641, "5-step half at step 5");
    assert!(!frames.irq_pending(), "mode 1 never sets the IRQ");
}

/// IRQ inhibit on `$4017` bit 6 keeps the flag unset through the run.
#[test]
fn irq_inhibit_suppresses_mode_zero_irq() {
    let mut frames = FrameCounter::new();
    frames.write(0x40); // inhibit
    let (_, halves) = drive(&mut frames, 16_000);
    assert_eq!(halves.len(), 2, "frame clocks are unaffected by inhibiting");
    assert!(!frames.irq_pending(), "inhibited IRQ stays clear");
}

/// A pulse riding the clocks: the envelope restarts on `$4003`, decays one
/// level per quarter frame at divider period 0, and the channel goes
/// silent once the envelope empties.
#[test]
fn pulse_reacts_to_frame_clocking() {
    let mut frames = FrameCounter::new();
    let mut pulse = Pulse::new(PulseChannel::One);
    pulse.set_enabled(true);
    pulse.write_ctrl(0x80); // duty 2, halt + constant-volume clear, period 0
    pulse.write_timer_lo(0x10);
    pulse.write_timer_hi(0x00); // length = 10, phase 0, envelope restart

    pulse.tick(); // step forwards off the reloaded duty phase
    pulse.clock_quarter_frame(); // envelope restart lands: counter = 15
    assert_eq!(pulse.level(), 15, "full volume at restart");

    let mut quarters = 0_u32;
    let mut silenced = None;
    for _ in 1..=70_000 {
        match frames.tick() {
            FrameEvent::Quarter => {
                pulse.clock_quarter_frame();
                quarters += 1;
                if quarters == 1 {
                    assert_eq!(
                        pulse.level(),
                        14,
                        "one envelope level per quarter at period 0"
                    );
                }
            }
            FrameEvent::Half => {
                pulse.clock_half_frame();
                if silenced.is_none() && pulse.level() == 0 {
                    silenced = Some(quarters);
                }
            }
            FrameEvent::None => {}
        }
    }
    assert!(
        silenced.is_some(),
        "decays to silence well inside the run, silenced after {silenced:?} quarters"
    );
}

fn assert_near(actual: u32, expected: u32, what: &str) {
    assert!(
        actual.abs_diff(expected) <= 1,
        "{what}: expected ~{expected}, got {actual}"
    );
}

/// Drives the frame counter, forwarding events like the music driver would.
fn drive(frames: &mut FrameCounter, ticks: u32) -> (Vec<u32>, Vec<u32>) {
    let mut quarters = Vec::new();
    let mut halves = Vec::new();
    for tick in 1..=ticks {
        match frames.tick() {
            FrameEvent::None => {}
            FrameEvent::Quarter => quarters.push(tick),
            FrameEvent::Half => halves.push(tick),
        }
    }
    (quarters, halves)
}
