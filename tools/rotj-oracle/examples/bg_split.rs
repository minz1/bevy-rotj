//! Watches the PPU's scroll/address registers (`$2000/$2005/$2006`) during the
//! `oracle-route` replay, to locate the code behind the top-band scroll split
//! `docs/LEVELDATA.md` §16 reports.
//!
//! Run with the ROM present: `cargo run -p rotj-oracle --example bg_split`.

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
    machine.bus.trace.watch.push(0x2000..=0x2007);
    let pads = inputs();
    for &pad in pads.iter().take(1001).skip(1) {
        machine.run_frame(pad);
    }
    // Per frame, list the `$2000`/`$2005`/`$2006` writes; the PC and bank say
    // whether they belong to the NMI (bank 15 `$E1D0-$E2FF`) or its flusher
    // (bank 14 `$C094-$C0B6`) or to main-loop code.
    let mut last = 0u64;
    for w in &machine.bus.trace.writes {
        if w.addr > 0x2006 || w.frame < 640 {
            continue;
        }
        if w.frame != last {
            println!("-- frame {}", w.frame);
            last = w.frame;
        }
        println!(
            "   ${:04X} = ${:02X}  pc=${:04X} bank={:?}",
            w.addr, w.value, w.pc, w.bank
        );
    }
}
