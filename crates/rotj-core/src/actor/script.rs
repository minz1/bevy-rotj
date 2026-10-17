//! A script as a table of rows run in order, each for one frame or more, or for none and
//! the next row runs in the same frame (`docs/specs/2026-10-17-scripts-as-data-design.md`).

/// How a row's frame went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// It waited for the NMI and runs again next frame.
    Waited,
    /// It has run; the next row runs in this frame.
    Done,
}

/// Where a script is in its table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Runner {
    /// The row running.
    pub at: usize,
    /// Frames the row has run.
    pub frame: u16,
}

impl Runner {
    /// The row running, or `None` once the table has run out.
    pub fn row<'a, A>(&self, table: &'a [A]) -> Option<&'a A> {
        table.get(self.at)
    }

    /// Ends the frame on [`Flow::Waited`] (true) or moves to the next row.
    pub fn advance(&mut self, flow: Flow) -> bool {
        match flow {
            Flow::Waited => {
                self.frame = self.frame.saturating_add(1);
                true
            }
            Flow::Done => {
                self.at += 1;
                self.frame = 0;
                false
            }
        }
    }
}

/// A row that waits `count` frames.
#[must_use]
pub fn wait(frame: u16, count: u16) -> Flow {
    if frame < count {
        Flow::Waited
    } else {
        Flow::Done
    }
}
