//! The oracle's picture against fceux's, every frame of the TAS from power-on to the
//! ending's hold. Each frame's lines are rendered through `rotj_core`'s renderer and must
//! have the digest of fceux's screenshot (`tests/data/tas-picture.txt`). A line a
//! mid-frame write splits shows one of its two states up to a pixel and the other after it
//! (`cut` lines in the table give which, from fceux's own pixels), so the check is exact.
//!
//! `ROTJ_RECORD_PICTURE=1` rewrites the table from the local capture
//! (`tools/emu/tas-shots/all.idx`: `tools/emu/dump.lua INPUT=replay SHOTS=1,48100,1`, then
//! `tools/emu/pack-shots.py tools/emu/tas-shots 1 48100 tools/emu/tas-shots/all.idx`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use rotj_core::digest::fnv1a;
use rotj_core::render::{Frame, Line, Picture};
use rotj_oracle::{FrameLog, Machine, power_on, read_inputs};

const LAST: u64 = 48100;
/// fceux's screenshot crops 8 lines top and bottom.
const SHOWN: std::ops::Range<usize> = 8 * 256..232 * 256;

/// A split line's picture: split `split` of the frame, its states in `order` (0: first
/// then second), switching at pixel `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cut {
    y: usize,
    split: usize,
    order: u8,
    x: usize,
}

fn line(l: &rotj_oracle::Line) -> Line {
    Line {
        v: l.v,
        fine_x: l.fine_x,
        ctrl: l.ctrl,
        mask: l.mask,
        chr: l.chr,
        sprite_chr: l.sprite_chr,
        mirroring: l.mirroring,
    }
}

fn render(m: &Machine, lines: &[Line]) -> Vec<u8> {
    let mut frame = Frame::new();
    frame.render(&Picture {
        lines,
        vram: &m.bus.ppu.vram,
        palette: &m.bus.ppu.palette,
        oam: &m.bus.ppu.oam,
        chr: &m.bus.cart.chr,
    });
    frame.indices()[SHOWN].to_vec()
}

/// The frame's rendering with each split line replaced by its two states' renders.
struct Rendered {
    plain: Vec<u8>,
    splits: Vec<(usize, Vec<u8>, Vec<u8>)>,
}

impl Rendered {
    fn new(m: &Machine, log: &FrameLog) -> Rendered {
        let lines: Vec<Line> = log.lines.iter().map(line).collect();
        let splits = log
            .splits
            .iter()
            .map(|(y, first, second)| {
                let (mut a, mut b) = (lines.clone(), lines.clone());
                (a[*y], b[*y]) = (line(first), line(second));
                (*y, render(m, &a), render(m, &b))
            })
            .collect();
        Rendered {
            plain: render(m, &lines),
            splits,
        }
    }

    fn row(y: usize) -> std::ops::Range<usize> {
        (y - 8) * 256..(y - 7) * 256
    }

    /// The picture with `cuts` applied.
    fn cut(&self, cuts: &[Cut]) -> Vec<u8> {
        let mut out = self.plain.clone();
        for cut in cuts {
            let (_, a, b) = &self.splits[cut.split];
            let (left, right) = if cut.order == 0 { (a, b) } else { (b, a) };
            let row = Rendered::row(cut.y);
            for x in 0..256 {
                out[row.start + x] = if x < cut.x { left } else { right }[row.start + x];
            }
        }
        out
    }

    /// The cuts that make the frame fceux's, or the rows nothing explains.
    fn explain(&self, theirs: &[u8]) -> Result<Vec<Cut>, Vec<usize>> {
        let (mut cuts, mut rows) = (Vec::new(), Vec::new());
        for y in 8..232 {
            let row = Rendered::row(y);
            let t = &theirs[row.clone()];
            if self.plain[row.clone()] == *t {
                continue;
            }
            let found = self
                .splits
                .iter()
                .enumerate()
                .filter(|(_, s)| s.0 == y)
                .find_map(|(split, (_, a, b))| {
                    (0..2).find_map(|order| {
                        let (left, right) = if order == 0 { (a, b) } else { (b, a) };
                        (0..=256)
                            .find(|&x| {
                                (0..256).all(
                                    |i| if i < x { left } else { right }[row.start + i] == t[i],
                                )
                            })
                            .map(|x| Cut { y, split, order, x })
                    })
                });
            match found {
                Some(cut) => cuts.push(cut),
                None => rows.push(y),
            }
        }
        if rows.is_empty() { Ok(cuts) } else { Err(rows) }
    }
}

fn table_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/tas-picture.txt")
}

/// The table's digests and cuts by frame.
fn read_table() -> (BTreeMap<u64, u64>, BTreeMap<u64, Vec<Cut>>) {
    let (mut digests, mut cuts) = (BTreeMap::new(), BTreeMap::<u64, Vec<Cut>>::new());
    for line in include_str!("data/tas-picture.txt").lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[..] {
            ["tas", frame, digest] => {
                digests.insert(
                    frame.parse().expect("frame"),
                    u64::from_str_radix(digest, 16).expect("digest"),
                );
            }
            ["cut", frame, y, split, order, x] => cuts
                .entry(frame.parse().expect("frame"))
                .or_default()
                .push(Cut {
                    y: y.parse().expect("y"),
                    split: split.parse().expect("split"),
                    order: order.parse().expect("order"),
                    x: x.parse().expect("x"),
                }),
            _ => {}
        }
    }
    (digests, cuts)
}

#[test]
#[ignore = "48100 frames (release, 60 s); the gates run it in release with --ignored"]
fn the_oracle_draws_every_tas_frame_like_fceux() {
    let Some(mut m) = power_on() else {
        return;
    };
    let record = std::env::var_os("ROTJ_RECORD_PICTURE").is_some();
    let (digests, cuts) = read_table();
    let inputs = read_inputs(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../emu/tas/inputs.csv"));
    let capture_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../emu/tas-shots/all.idx");
    let mut capture = std::fs::File::open(&capture_path).ok();
    assert!(
        !record || capture.is_some(),
        "recording needs {}",
        capture_path.display()
    );
    let mut table = String::from(
        "# fceux 2.6.5's picture of every TAS frame from power-on (lines 8-231 of its indexed\n\
         # screenshot, FNV-1a 64), and for a line a mid-frame write splits, where fceux switches\n\
         # between its two states (`cut frame y split order x`). Written by\n\
         # ROTJ_REQUIRE_ROM=1 ROTJ_RECORD_PICTURE=1 cargo test --release -p rotj-oracle --test picture -- --ignored\n",
    );
    let (mut drawn, mut cut_frames, mut failures) = (0, 0, Vec::new());
    let mut theirs = vec![0; SHOWN.len()];
    for frame in 1..=LAST {
        let pad = inputs
            .get(usize::try_from(frame).expect("small"))
            .copied()
            .unwrap_or(0);
        let log = m.run_frame(pad);
        if let Some(file) = &mut capture {
            file.seek(SeekFrom::Start((frame - 1) * SHOWN.len() as u64))
                .expect("seek");
            file.read_exact(&mut theirs).expect("the capture's frame");
        }
        if record {
            writeln!(table, "tas {frame} {:016x}", fnv1a(&theirs)).expect("string");
        }
        if log.lines.is_empty() {
            continue;
        }
        drawn += 1;
        let rendered = Rendered::new(&m, &log);
        if record {
            match rendered.explain(&theirs) {
                Ok(found) => {
                    cut_frames += usize::from(!found.is_empty());
                    for c in found {
                        writeln!(table, "cut {frame} {} {} {} {}", c.y, c.split, c.order, c.x)
                            .expect("string");
                    }
                }
                Err(rows) => failures.push(format!("frame {frame}: rows {rows:?} unexplained")),
            }
            continue;
        }
        let frame_cuts = cuts.get(&frame).map_or(&[][..], Vec::as_slice);
        cut_frames += usize::from(!frame_cuts.is_empty());
        if fnv1a(&rendered.cut(frame_cuts)) == digests[&frame] {
            continue;
        }
        let detail = if capture.is_some() {
            format!("{:?}", rendered.explain(&theirs).err().unwrap_or_default())
        } else {
            "no local capture for rows".to_owned()
        };
        failures.push(format!("frame {frame}: differs from fceux, rows {detail}"));
    }
    if record {
        std::fs::write(table_path(), table).expect("table written");
    }
    println!("{drawn} frames drawn, {cut_frames} with a split line");
    assert_eq!(
        m.bus.ppu.unmodelled, 0,
        "writes the line model does not represent"
    );
    assert!(
        failures.is_empty(),
        "{} frames differ from fceux (first {:?}); to see the pixels, regenerate the capture (module doc)",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
}
