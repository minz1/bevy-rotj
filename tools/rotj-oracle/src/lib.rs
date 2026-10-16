//! Dev-only 6502 oracle: the game's own frames from power-on, for diffing the port in tests.

mod bus;
mod cart;
mod cpu;
mod fme7;
mod machine;
mod ppu;
mod replay;

pub use bus::{
    Dmc, ExecEvent, NesBus, PeekEvent, Playtest, StartBytes, Trace, WriteEvent, WriteLog,
};
pub use cart::{Cart, load_rom};
pub use cpu::Cpu;
pub use fme7::Fme7;
pub use machine::{FrameLog, Machine};
pub use ppu::{Line, Ppu};
pub use replay::{data_path, physics_input, plan_input, power_on, read_inputs};
