//! Shared data formats for the Batman: Return of the Joker (NES, Sunsoft FME-7)
//! port. All code is safe Rust; `unsafe` is denied at the workspace level.

pub mod actor;
pub mod actors;
pub mod chr;
pub mod collision;
pub mod digest;
pub mod ines;
pub mod level;
pub mod levelmap;
pub mod palette;
pub mod ppulist;
pub mod prg;
pub mod render;
pub mod rooms;
pub mod stage;
pub mod sublevels;
pub mod terrain;

pub use chr::{TILE_SHEET_H, TILE_SHEET_W, TILES_PER_BANK};
pub use ines::{GRAY_PREVIEW_PALETTE, InesHeader, InesRom};
pub use ppulist::{Entry, WriteList};
