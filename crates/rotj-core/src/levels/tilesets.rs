//! The tilesets a pack's maps name, one per area and CHR variant, drawn from the player's
//! ROM as previews for the editor.

use super::model::{Property, to_text};
use super::{Catalog, JSON_VERSION, Levels, TILED_VERSION, TILESET_TILES, tileset_name};
use crate::actor::room::{build_palette, loaded_bank};
use crate::actor::world::set_first_colours;
use crate::chr;
use crate::palette::rgba;
use crate::rooms::Room;
use crate::terrain::Terrain;

/// A tileset picture's width and height in px: 16 metatiles of 16 px.
pub const TILESET_PX: u32 = 256;

/// A tileset a pack's maps name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tileset {
    /// Its name, `area-06-b`.
    pub name: String,
    /// Its `.tsj` file's text.
    pub text: String,
    /// Its picture, RGBA, [`TILESET_PX`] square: metatile `id` at column `id % 16`, row
    /// `id / 16`, clear past the area's metatiles.
    pub picture: Vec<u8>,
}

/// Every catalog area's tilesets, in catalog order; `None` where the ROM's levels do not
/// import.
#[must_use]
pub fn tilesets(prg: &[u8], chr: &[u8]) -> Option<Vec<Tileset>> {
    let levels = Levels::from_rom(prg, chr)?;
    let catalog = Catalog::from_levels(&levels);
    let mut tilesets = Vec::new();
    for (area, stage) in catalog.areas.iter().zip(&levels.stages) {
        for (variant, &banks) in area.variants.iter().enumerate() {
            let first_room = stage
                .rooms
                .iter()
                .map(|assets| &assets.room)
                .find(|room| room.chr == banks)
                .expect("a variant is a room's");
            let name = tileset_name(area.area, variant);
            tilesets.push(Tileset {
                text: tsj(&name, banks),
                picture: picture(prg, chr, &area.terrain, first_room),
                name,
            });
        }
    }
    Some(tilesets)
}

fn tsj(name: &str, banks: [u8; 4]) -> String {
    to_text(&serde_json::json!({
        "columns": 16,
        "image": format!("{name}.png"),
        "imageheight": TILESET_PX,
        "imagewidth": TILESET_PX,
        "margin": 0,
        "name": name,
        "properties": [Property::int_list("chr", banks.map(i64::from))],
        "spacing": 0,
        "tilecount": TILESET_TILES,
        "tiledversion": TILED_VERSION,
        "tileheight": 16,
        "tilewidth": 16,
        "type": "tileset",
        "version": JSON_VERSION,
    }))
}

/// `terrain`'s metatiles in the banks and background colours `room` loads with.
fn picture(prg: &[u8], chr: &[u8], terrain: &Terrain, room: &Room) -> Vec<u8> {
    let mut palette = [0; 32];
    build_palette(
        prg,
        room.background_palette,
        room.sprite_palette,
        &mut palette,
    );
    set_first_colours(&mut palette, 0x0F);
    let banks = room.chr.map(|bank| loaded_bank(prg, bank));
    let side = TILESET_PX as usize;
    let mut picture = vec![0; side * side * 4];
    for (id, corners) in terrain.corners.iter().enumerate() {
        let colours = &palette[4 * usize::from(terrain.palette_classes[id])..][..4];
        for (corner, &tile) in corners.iter().enumerate() {
            let pattern = chr::tile(chr, &banks, usize::from(tile) * chr::TILE_BYTES);
            draw_corner(&mut picture, id, corner, pattern, colours);
        }
    }
    picture
}

/// Draws `pattern` in `colours` as corner `corner` (top-left, top-right, bottom-left,
/// bottom-right) of metatile `id` in a [`TILESET_PX`]-wide RGBA `picture`.
pub(super) fn draw_corner(
    picture: &mut [u8],
    id: usize,
    corner: usize,
    pattern: &[u8],
    colours: &[u8],
) {
    let side = TILESET_PX as usize;
    for y in 0..8 {
        for x in 0..8 {
            let colour = colours[usize::from(chr::pixel(pattern, x, y))];
            let column = id % 16 * 16 + corner % 2 * 8 + x;
            let row = id / 16 * 16 + corner / 2 * 8 + y;
            picture[(row * side + column) * 4..][..4].copy_from_slice(&rgba(colour));
        }
    }
}
