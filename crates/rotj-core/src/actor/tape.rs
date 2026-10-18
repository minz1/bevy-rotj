//! State tapes: what a lockstep run injected and the state hashes it checked, for replay without the oracle.

use super::oracle::SCRATCH;
use crate::digest::fnv1a;

const MAGIC: u32 = 0x5441_5045;
const VERSION: u32 = 1;
/// RAM then WRAM, the space a tape's addresses index.
pub(crate) const MEMORY: usize = 0x2800;

/// Bytes written over the RAM+WRAM space: RAM at `0..0x800`, WRAM at `0x800..0x2800`.
pub(crate) type Writes = Vec<(u16, u8)>;

/// One thing the lockstep did to or checked on the port's game, in the order it did it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// The first lift: the oracle's RAM+WRAM.
    Start(Box<[u8; MEMORY]>),
    /// The interrupt bytes kept since the last injection, before the `$0181` filter.
    Irq(Writes),
    /// The bytes the NMI wait changed.
    Nmi(Writes),
    /// [`script_hash`] of a script frame.
    ScriptCheck(u64),
    /// [`loop_hash`] of a loop frame.
    Check(u64),
    /// GAME OVER reached the reset; the run ends.
    Reset,
    /// A draw hung both games; the run ends.
    Hung,
    /// The frame budget ran out; the run ends.
    End,
}

/// A lockstep run's events, each with the oracle frame it happened in.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Tape {
    pub events: Vec<(u64, Event)>,
}

fn clear_scratch(ram: &mut [u8]) {
    for range in &SCRATCH {
        ram[usize::from(*range.start())..=usize::from(*range.end())].fill(0);
    }
}

/// The hash of what a loop frame compares: RAM then WRAM, scratch zeroed.
pub(crate) fn loop_hash(ram: &[u8; 2048], wram: &[u8; 8192]) -> u64 {
    let mut memory = [0; MEMORY];
    memory[..0x800].copy_from_slice(ram);
    memory[0x800..].copy_from_slice(wram);
    clear_scratch(&mut memory);
    fnv1a(&memory)
}

/// The hash of what a script frame compares: RAM alone, scratch zeroed.
pub(crate) fn script_hash(ram: &[u8; 2048]) -> u64 {
    let mut ram = *ram;
    clear_scratch(&mut ram);
    fnv1a(&ram)
}

impl Event {
    /// The variant's wire tag and its name, in one place so the two cannot drift.
    fn tag_and_name(&self) -> (u8, &'static str) {
        match self {
            Event::Start(_) => (0, "Start"),
            Event::Irq(_) => (2, "Irq"),
            Event::Nmi(_) => (3, "Nmi"),
            Event::ScriptCheck(_) => (5, "ScriptCheck"),
            Event::Check(_) => (6, "Check"),
            Event::Reset => (7, "Reset"),
            Event::Hung => (8, "Hung"),
            Event::End => (9, "End"),
        }
    }

    /// The variant's name, for the stale-tape message.
    pub(crate) fn name(&self) -> &'static str {
        self.tag_and_name().1
    }

    fn tag(&self) -> u8 {
        self.tag_and_name().0
    }
}

fn put_writes(out: &mut Vec<u8>, writes: &Writes) {
    let count = u32::try_from(writes.len()).expect("fewer than 2^32 writes");
    out.extend(count.to_le_bytes());
    for &(addr, value) in writes {
        out.extend(addr.to_le_bytes());
        out.push(value);
    }
}

impl Tape {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(MAGIC.to_le_bytes());
        out.extend(VERSION.to_le_bytes());
        for (frame, event) in &self.events {
            out.push(event.tag());
            out.extend(frame.to_le_bytes());
            match event {
                Event::Start(memory) => out.extend_from_slice(&memory[..]),
                Event::Irq(writes) | Event::Nmi(writes) => put_writes(&mut out, writes),
                Event::ScriptCheck(hash) | Event::Check(hash) => out.extend(hash.to_le_bytes()),
                Event::Reset | Event::Hung | Event::End => {}
            }
        }
        out
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Tape, String> {
        let mut reader = Reader { bytes };
        let magic = u32::from_le_bytes(reader.array()?);
        if magic != MAGIC {
            return Err(format!("not a tape: magic {magic:#010X}"));
        }
        let version = u32::from_le_bytes(reader.array()?);
        if version != VERSION {
            return Err(format!(
                "tape version {version}, expected {VERSION}: re-record"
            ));
        }
        let mut events = Vec::new();
        while !reader.bytes.is_empty() {
            let [tag] = reader.array()?;
            let frame = u64::from_le_bytes(reader.array()?);
            let event = match tag {
                0 => Event::Start(Box::new(reader.array()?)),
                2 => Event::Irq(reader.writes()?),
                3 => Event::Nmi(reader.writes()?),
                5 => Event::ScriptCheck(u64::from_le_bytes(reader.array()?)),
                6 => Event::Check(u64::from_le_bytes(reader.array()?)),
                7 => Event::Reset,
                8 => Event::Hung,
                9 => Event::End,
                other => return Err(format!("event {} has an unknown tag {other}", events.len())),
            };
            events.push((frame, event));
        }
        match events.last() {
            Some((_, Event::Reset | Event::Hung | Event::End)) => Ok(Tape { events }),
            _ => Err("truncated tape: it ends before the run does".to_owned()),
        }
    }
}

/// The bytes of a tape not decoded yet.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let (head, rest) = self.bytes.split_first_chunk::<N>().ok_or_else(|| {
            format!(
                "truncated tape: {} bytes left, {N} needed",
                self.bytes.len()
            )
        })?;
        self.bytes = rest;
        Ok(*head)
    }

    fn writes(&mut self) -> Result<Writes, String> {
        let count = u32::from_le_bytes(self.array()?);
        (0..count)
            .map(|_| {
                let [low, high, value] = self.array()?;
                Ok((u16::from_le_bytes([low, high]), value))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(fill: u8) -> Box<[u8; 0x2800]> {
        Box::new([fill; 0x2800])
    }

    fn every_event() -> Tape {
        let writes = vec![(0x0000, 1), (0x27FF, 0xFF)];
        Tape {
            events: vec![
                (351, Event::Start(snapshot(1))),
                (352, Event::Irq(writes.clone())),
                (353, Event::Irq(Vec::new())),
                (354, Event::Nmi(writes)),
                (357, Event::ScriptCheck(0x0123_4567_89AB_CDEF)),
                (358, Event::Check(u64::MAX)),
                (359, Event::Reset),
                (360, Event::Hung),
                (u64::MAX, Event::End),
            ],
        }
    }

    #[test]
    fn scratch_bytes_do_not_change_the_hash() {
        let (zero, wram) = ([0; 2048], [0; 8192]);
        let mut scratch = zero;
        scratch[0x10] = 1;
        scratch[0x4A0] = 7;
        assert_eq!(loop_hash(&scratch, &wram), loop_hash(&zero, &wram));
        let mut live = zero;
        live[0x40] = 1;
        assert_ne!(loop_hash(&live, &wram), loop_hash(&zero, &wram));
    }

    #[test]
    fn script_hash_sees_the_mask_shadow() {
        let zero = [0; 2048];
        let mut mask = zero;
        mask[0xFE] = 0x1E;
        assert_ne!(script_hash(&mask), script_hash(&zero));
    }

    #[test]
    fn a_wram_only_change_changes_the_loop_hash() {
        let (ram, zero) = ([0; 2048], [0; 8192]);
        let mut wram = zero;
        wram[0x100] = 1;
        assert_ne!(loop_hash(&ram, &wram), loop_hash(&ram, &zero));
    }

    #[test]
    fn every_event_round_trips() {
        let tape = every_event();
        assert_eq!(Tape::decode(&tape.encode()), Ok(tape));
    }

    #[test]
    fn decode_rejects_a_wrong_magic_and_a_truncated_file() {
        let bytes = every_event().encode();
        let mut wrong = bytes.clone();
        wrong[0] ^= 1;
        assert!(Tape::decode(&wrong).is_err_and(|e| e.contains("magic")));
        let short = &bytes[..bytes.len() - 1];
        assert!(Tape::decode(short).is_err_and(|e| e.contains("truncated")));
        let mut unfinished = every_event();
        unfinished.events.truncate(6);
        let unfinished = unfinished.encode();
        assert!(Tape::decode(&unfinished).is_err_and(|e| e.contains("truncated")));
    }

    /// 1 and 4 were the retired `Relift` and `Outside`; 10 was never a tag.
    #[test]
    fn decode_rejects_an_unknown_tag() {
        for tag in [1, 4, 10] {
            let mut bytes = Vec::new();
            bytes.extend(MAGIC.to_le_bytes());
            bytes.extend(VERSION.to_le_bytes());
            bytes.push(tag);
            bytes.extend(0u64.to_le_bytes());
            assert!(
                Tape::decode(&bytes).is_err_and(|e| e.contains("unknown tag")),
                "tag {tag}"
            );
        }
    }

    #[test]
    fn decode_rejects_a_huge_write_count_without_allocating() {
        let mut bytes = Vec::new();
        bytes.extend(MAGIC.to_le_bytes());
        bytes.extend(VERSION.to_le_bytes());
        bytes.push(2); // Irq
        bytes.extend(0u64.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes()); // more writes than the rest can hold
        bytes.extend([0, 0]); // not even one 3-byte write
        assert!(Tape::decode(&bytes).is_err_and(|e| e.contains("truncated")));
    }
}
