//! Audit (TASK=cutaudit): song id `$06` is defined by the driver's song table
//! (bank 10 `$8B91`/`$8B92` -> bank 6 header index 5, header `$B5D1`) but no
//! game code requests it. It is the "Unused [Track 6]" of the NSF rip. This
//! test proves the id is a real, looping song rather than a stub: it plays it,
//! reports the register writes per channel, and finds the period its pulse 1
//! note sequence settles into (the song stream's own loop, bank 6 `$B5E5`).
//!
//! It does not change the driver; it only selects a different song id.

use rotj_music::Music;

/// The id no game code requests (see `docs/CUTCONTENT.md`).
const UNUSED: u8 = 6;
/// How long to play for the per-channel write tally.
const TALLY_FRAMES: usize = 600;
/// How long to play to observe the loop.
const LOOP_FRAMES: usize = 12000;
/// The longest loop period looked for, in frames.
const MAX_PERIOD: usize = 2000;
/// The tail of the run the period is checked over.
const LOOP_WINDOW: usize = 6000;

/// Which APU channel a register belongs to, for the per-channel tally.
fn channel(addr: u16) -> usize {
    match addr {
        0x4000..=0x4003 => 0, // pulse 1
        0x4004..=0x4007 => 1, // pulse 2
        0x4008..=0x400B => 2, // triangle
        0x400C..=0x400F => 3, // noise
        0x4010..=0x4013 => 4, // DMC
        _ => 5,               // $4015 status, and anything else
    }
}

fn music() -> Option<Music> {
    Some(Music::from_prg(&rotj_rom::for_test()?.prg))
}

#[test]
fn unused_song_plays_and_loops() {
    let names = ["pulse1", "pulse2", "triangle", "noise", "dmc", "status"];
    let Some(mut music) = music() else {
        return;
    };
    music.init();
    music.request(UNUSED);

    let mut counts = [0_usize; 6];
    for _ in 0..TALLY_FRAMES {
        for w in music.tick() {
            counts[channel(w.addr)] += 1;
        }
    }
    println!("song id ${UNUSED:02X} over {TALLY_FRAMES} frames:");
    for (name, n) in names.iter().zip(&counts) {
        println!("  {name:>8}: {n} writes");
    }
    let sounded = counts[0] + counts[1] + counts[2] + counts[3];
    assert!(sounded > 0, "song ${UNUSED:02X} produced no channel writes");

    // Note-trigger signature per frame for pulse 1 ($4003), 0xFF = no trigger.
    let mut sig = vec![0xFF_u8; LOOP_FRAMES];
    for slot in &mut sig {
        for w in music.tick() {
            if w.addr == 0x4003 {
                *slot = w.value;
            }
        }
    }

    // The song streams loop (`$FA`/`$FB` in bank 6); the pulse 1 line is an
    // ostinato, so find the smallest period the run's tail repeats with.
    let window_start = LOOP_FRAMES - LOOP_WINDOW;
    let period = (16..=MAX_PERIOD)
        .find(|&p| {
            (window_start..LOOP_FRAMES)
                .filter(|&i| sig[i] == sig[i - p])
                .count()
                * 100
                >= LOOP_WINDOW * 98
        })
        .expect("pulse 1 sequence never repeats: is it looping?");
    assert_eq!(period, 176, "pulse 1 loop period");
    let secs = f64::from(u16::try_from(period).expect("period fits in u16")) / 60.0;
    println!(
        "pulse 1 loop: period {period} frames ({secs:.2}s), >=98% of the last {LOOP_WINDOW} frames repeat"
    );
}
