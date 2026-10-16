//! Level loads with `$20` forced at bank 14 `$D3D8`, as `tools/emu/stagearea-probe` does.

pub mod common;

use rotj_core::digest::{fnv1a, table};
use rotj_core::levelmap::{area_for_level, map_from_rom};
use rotj_oracle::power_on;
use rotj_oracle::{Machine, WriteEvent};

const LOADED: u64 = 610;
const SETTLED: u64 = 650;
/// The frame the forced load's `$6000-$6FFF` map is complete at (the frame-624 dump).
const MAPPED: u64 = 623;

/// A committed FNV-1a digest of an fceux capture (`tests/data/digests.txt`).
fn digest(name: &str) -> u64 {
    table(include_str!("data/digests.txt"), name)["-"]
}

/// `$20` forced, the map and actor types (`$7CC0-$7CDF`) watched; Start at 300-306.
fn forced(level: u8) -> Option<Machine> {
    let mut machine = power_on()?;
    machine.bus.trace.pokes.push((0xD3D8, 0x0020, level));
    machine.bus.trace.watch.push(0x6000..=0x6FFF);
    machine.bus.trace.watch.push(0x7CC0..=0x7CDF);
    Some(machine)
}

fn run_to(machine: &mut Machine, frame: u64) {
    while machine.frame < frame {
        let start = (300..=306).contains(&(machine.frame + 1));
        machine.run_frame(if start { 0x08 } else { 0x00 });
    }
}

fn rom_map(machine: &Machine, level: u8) -> Vec<u8> {
    let area = area_for_level(usize::from(level)).expect("level index");
    let map = map_from_rom(&machine.bus.cart.prg, &machine.bus.cart.chr, area).expect("area map");
    map.iter().flatten().copied().collect()
}

/// The addresses in `$6000-$6FFF` whose column (`addr % 256`) is in `range`.
fn columns(range: std::ops::Range<usize>) -> impl Iterator<Item = usize> {
    (0..0x1000).filter(move |i| range.contains(&(i % 256)))
}

#[test]
fn stages_2_2_and_5_2_are_one_screen_maps_the_level_actor_clears() {
    for level in [3, 9] {
        let Some(mut machine) = forced(level) else {
            return;
        };
        run_to(&mut machine, LOADED);
        let map = rom_map(&machine, level);
        assert!(
            columns(0..16).all(|i| machine.bus.wram[i] == map[i]),
            "level {level}"
        );

        run_to(&mut machine, SETTLED);
        assert!(
            columns(0..16).all(|i| machine.bus.wram[i] == map[i]),
            "level {level}"
        );
        assert!(
            columns(16..256).all(|i| machine.bus.wram[i] == 0),
            "level {level}"
        );
        let clears: Vec<&WriteEvent> = machine
            .bus
            .trace
            .writes
            .iter()
            .filter(|w| w.frame > LOADED && w.addr < 0x7000)
            .collect();
        assert_eq!(clears.len(), 240 * 16);
        assert!(
            clears
                .iter()
                .all(|w| (w.pc, w.bank, w.value) == (0x9E16, Some(11), 0))
        );
        assert_eq!(
            (clears[0].frame, clears[clears.len() - 1].frame),
            (635, 649)
        );
        let spawn = machine
            .bus
            .trace
            .writes
            .iter()
            .find(|w| w.addr >= 0x7CC0 && w.value == 0x56);
        let spawn = spawn.map(|w| (w.frame, w.pc, w.bank, w.addr));
        assert_eq!(
            spawn,
            Some((601, 0xD7A8, Some(14), 0x7CC1)),
            "level {level}"
        );
    }
}

#[test]
fn a_full_stage_keeps_its_map() {
    let Some(mut machine) = forced(2) else { return };
    run_to(&mut machine, SETTLED);
    assert_eq!(&machine.bus.wram[..0x1000], &rom_map(&machine, 2)[..]);
    assert!(!machine.bus.trace.writes.iter().any(|w| w.pc == 0x9E16));
}

/// Every level 0-12's forced load matches the ROM map for its area. The three
/// levels with a committed capture (`tools/emu/stagearea-probe`, `sublevels-force`)
/// also match fceux's bytes by digest. One-screen areas (levels 3, 9) decode 16
/// metatile columns; the level actor clears the payload columns after the load,
/// so only their real columns are level data.
#[test]
fn every_level_matches_the_rom_map() {
    for level in 0..13u8 {
        let Some(mut machine) = forced(level) else {
            return;
        };
        run_to(&mut machine, MAPPED);
        let map = rom_map(&machine, level);
        let real = if matches!(level, 3 | 9) { 16 } else { 256 };
        let diff = columns(0..real)
            .filter(|&i| machine.bus.wram[i] != map[i])
            .count();
        assert_eq!(diff, 0, "level {level}: forced load vs ROM map");
        let name = match level {
            1 => Some("area1-map"),
            4 => Some("area5-map"),
            12 => Some("area16-map"),
            _ => None,
        };
        if let Some(name) = name {
            assert_eq!(
                fnv1a(&machine.bus.wram[..0x1000]),
                digest(name),
                "level {level}: the oracle's whole map differs from fceux's"
            );
        }
    }
}
