//! Reverse-engineering evidence for the background CHR animation
//! (`docs/LEVELDATA.md` §16): replays `oracle-route` and prints the writes to
//! the CHR shadow `$E0-$E7` and the animation's own counters
//! (`$0430-$0433`, sequence `$0410`) for the slot that moves.
//!
//! Run with the ROM present: `cargo run -p rotj-oracle --example bg_chr`.

use std::path::PathBuf;

use rotj_oracle::{Machine, load_rom};

fn inputs() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/route-inputs.csv");
    let text = std::fs::read_to_string(path).expect("route-inputs.csv");
    text.lines()
        .skip(1)
        .map(|line| {
            line.split_once(',')
                .expect("frame,pad")
                .1
                .parse()
                .expect("pad")
        })
        .collect()
}

fn main() {
    let Some(cart) = load_rom() else {
        return;
    };
    let mut machine = Machine::power_on(cart);
    machine.bus.trace.watch.push(0x00E0..=0x00E7);
    let pads = inputs();
    for &pad in pads.iter().take(701).skip(1) {
        machine.run_frame(pad);
    }

    println!("== writes to $E3 (slot 3), frames 560-700 ==");
    for w in machine
        .bus
        .trace
        .writes
        .iter()
        .filter(|w| w.addr == 0x00E3 && w.frame >= 560)
    {
        println!(
            "f{} ${:04X}=${:02X} pc=${:04X} bank={:?}",
            w.frame, w.addr, w.value, w.pc, w.bank
        );
    }

    // Slot 3's record: period `$0430+12`, count `$0431+12`, tick `$0432+12`,
    // index `$0433+12`, sequence `$0410+24`.
    let at = 12;
    let seq: Vec<String> = (0..8)
        .map(|j| format!("${:02X}", machine.bus.ram[0x0410 + 24 + j]))
        .collect();
    println!(
        "== slot 3 record: period ${:02X} count ${:02X} tick ${:02X} index ${:02X} seq {}",
        machine.bus.ram[0x0430 + at],
        machine.bus.ram[0x0431 + at],
        machine.bus.ram[0x0432 + at],
        machine.bus.ram[0x0433 + at],
        seq.join(" "),
    );
}
