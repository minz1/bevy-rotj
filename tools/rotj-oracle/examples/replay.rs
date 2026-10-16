//! Replays an `inputs.csv` from power-on, printing level progress; optionally dumps RAM at one frame.
//!
//! `cargo run -p rotj-oracle --release --example replay -- <inputs.csv> [every] [dump_frame]`

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .expect("usage: replay <inputs.csv> [every] [dump_frame]");
    let every: usize = args.get(2).map_or(600, |s| s.parse().expect("every"));
    let dump: Option<usize> = args.get(3).map(|s| s.parse().expect("dump_frame"));
    let inputs = rotj_oracle::read_inputs(Path::new(path));
    let mut machine = rotj_oracle::power_on().expect("ROM at raw/");
    let mut last = None;
    for (frame, &pad) in inputs.iter().enumerate().skip(1) {
        machine.run_frame(pad);
        let ram = &machine.bus.ram;
        if dump == Some(frame) {
            std::fs::write(format!("ram-{frame}.bin"), ram).expect("write RAM dump");
        }
        let level = Some((ram[0x400], ram[0x402]));
        if frame % every == 0 || level != last {
            println!(
                "frame {frame:6} $0400={:02X} $0402={:02X} lives={:02X} energy={:02X} $BA={:02X}",
                ram[0x400], ram[0x402], ram[0x482], ram[0x478], ram[0xBA]
            );
            last = level;
        }
    }
}
