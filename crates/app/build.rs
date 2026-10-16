//! Embeds the user's ROM (`ROTJ_ROM`, else the workspace's `raw/`) after checking it; without
//! one the exe is built to say so and exit.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-env-changed=ROTJ_ROM");
    println!("cargo::rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let (prg, chr) = match rotj_rom::locate() {
        Err(error) => panic!("{error}"),
        Ok(Some((path, rom))) => {
            watch(&path);
            (rom.prg, rom.chr)
        }
        Ok(None) => {
            if let Some(raw) = rotj_rom::raw_path().parent().filter(|raw| raw.is_dir()) {
                watch(raw);
            }
            println!(
                "cargo::warning=built without a ROM: set ROTJ_ROM or put it in raw/ and rebuild"
            );
            (Vec::new(), Vec::new())
        }
    };
    std::fs::write(out.join("prg.bin"), prg).expect("prg.bin");
    std::fs::write(out.join("chr.bin"), chr).expect("chr.bin");
}

fn watch(path: &Path) {
    let path = path.to_str().expect("a UTF-8 ROM path");
    println!("cargo::rerun-if-changed={path}");
}
