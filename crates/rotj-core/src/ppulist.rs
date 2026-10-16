//! PPU write-lists: the `[hi, lo, count, bytes...]` streams the game feeds to
//! `$2006`/`$2007` to fill nametables and palettes (`disasm/MAP.md` §4/§5).
//!
//! A list is a sequence of entries, each `hi, lo` (a PPU address, big-endian
//! like the PPU's own address latch), `count`, then `count` data bytes. `$FF`
//! where a new entry's `hi` would be terminates the list.
//!
//! Format recovered from the ROM bytes and confirmed against two independent
//! derivations: the lists at bank 02 `$91B7` and bank 0C `$9BEB` end exactly
//! at offsets `$1203` and `$1C35`, the extents the disassembly recorded for
//! them. [`crate::ppulist::verify_known_lists`] re-checks that.

/// Iterator over the entries of one write-list.
#[derive(Debug, Clone)]
pub struct WriteList<'a> {
    data: &'a [u8],
    pos: usize,
}

/// One decoded entry: a PPU address and the bytes written to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// Destination, e.g. `$2000`-range for nametables or `$3F00` for palettes.
    pub addr: u16,
    /// The bytes written starting at that address.
    pub data: &'a [u8],
}

impl<'a> WriteList<'a> {
    /// Walk the list starting at the beginning of `data`.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Offset just past the terminating `$FF`, or `None` if the list never
    /// terminated within `data`.
    #[must_use]
    pub const fn end_offset(&self) -> Option<usize> {
        if self.pos < self.data.len() && self.data[self.pos] == 0xFF {
            Some(self.pos + 1)
        } else {
            None
        }
    }
}

impl<'a> Iterator for WriteList<'a> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Entry<'a>> {
        let hi = *self.data.get(self.pos)?;
        if hi == 0xFF {
            return None;
        }
        let lo = *self.data.get(self.pos + 1)?;
        let count = usize::from(*self.data.get(self.pos + 2)?);
        let start = self.pos + 3;
        let end = start.checked_add(count)?;
        let bytes = self.data.get(start..end)?;
        self.pos = end;
        Some(Entry {
            addr: u16::from(hi) << 8 | u16::from(lo),
            data: bytes,
        })
    }
}

/// A known list to re-check against the ROM: `(bank, offset, entries, address
/// of the terminating $FF)`. The values come from `disasm/MAP.md` §4/§5, which
/// recorded where each list ends.
pub const KNOWN_LISTS: [(u8, u16, usize, u16); 2] = [
    // bank, base offset, entries, terminator offset
    (0x02, 0x11B7, 6, 0x1203),
    (0x0C, 0x1BEB, 8, 0x1C35),
];

/// Decode the list at `bank`:`offset` and report whether it matches the
/// recorded shape. Returns a human-readable summary for either outcome.
#[must_use]
pub fn verify_known_lists(prg: &[u8]) -> Vec<String> {
    KNOWN_LISTS
        .iter()
        .map(|&(bank, offset, entries, terminator)| {
            let base = usize::from(bank) * crate::ines::FME7_PRG_BANK_SIZE + usize::from(offset);
            let slice = prg.get(base..).unwrap_or(&[]);
            let mut list = WriteList::new(slice);
            let decoded: Vec<Entry<'_>> = list.by_ref().collect();
            // `end_offset` is one past the $FF, so the terminator sits at
            // `terminator - offset`.
            let terminator_at = list
                .end_offset()
                .and_then(|end| u16::try_from(end).ok())
                .map(|end| offset + end - 1);
            let ok = decoded.len() == entries && terminator_at == Some(terminator);
            format!(
                "bank {bank:02X} ${offset:04X}: {} entries (expected {entries}), \
                 terminator ${:04X} (expected ${terminator:04X}) -> {}",
                decoded.len(),
                terminator_at.unwrap_or(0),
                if ok { "OK" } else { "MISMATCH" }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_entries_and_finds_the_terminator() {
        // Two entries then $FF: $2000 <- 01 02, $3F00 <- 0F.
        let data = [
            0x20, 0x00, 0x02, 0x01, 0x02, 0x3F, 0x00, 0x01, 0x0F, 0xFF, 0x00,
        ];
        let mut list = WriteList::new(&data);
        let first = list.next().expect("first entry");
        assert_eq!(first.addr, 0x2000);
        assert_eq!(first.data, [0x01, 0x02]);
        let second = list.next().expect("second entry");
        assert_eq!(second.addr, 0x3F00);
        assert_eq!(second.data, [0x0F]);
        assert!(list.next().is_none(), "terminator stops the list");
        assert_eq!(list.end_offset(), Some(10));
    }

    #[test]
    fn truncated_lists_end_instead_of_panicking() {
        // Claims two bytes but only one follows.
        let data = [0x20, 0x00, 0x02, 0x01];
        assert!(WriteList::new(&data).next().is_none());
        // No terminator at all.
        let data = [0x20, 0x00, 0x01, 0x01];
        let mut list = WriteList::new(&data);
        assert!(list.next().is_some());
        assert_eq!(list.end_offset(), None);
    }

    #[test]
    fn entry_can_run_to_the_end_of_the_slice() {
        let data = [0x20, 0x00, 0x02, 0x01, 0x02];
        let mut list = WriteList::new(&data);
        assert_eq!(list.next().map(|e| e.addr), Some(0x2000));
        assert!(list.next().is_none());
    }
}
