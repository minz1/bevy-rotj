//! A pack's custom metatiles, `custom.tsj`: a Tiled tileset whose tiles name their corners'
//! CHR banks and tiles, their palette class and their surface.

use std::collections::BTreeSet;

use serde::Deserialize;

use super::Catalog;
use super::import::Metatile;
use super::model::Property;
use super::tilesets::{TILESET_PX, draw_corner};
use crate::actor::room::loaded_bank;
use crate::chr;
use crate::collision::CLASS_COUNT;

/// The tileset name a room's map gives it.
pub(crate) const CUSTOM: &str = "custom";
/// Its file in a pack.
pub(crate) const CUSTOM_FILE: &str = "custom.tsj";

/// The `surface` enum's names and the collision class each writes (a name covering several
/// classes is its lowest).
pub(crate) const SURFACES: [(&str, u8); 13] = [
    ("solid", 0),
    ("belt-right", 1),
    ("belt-left", 2),
    ("hazard-behind", 3),
    ("solid-hazard", 4),
    ("slippery", 5),
    ("behind", 6),
    ("open", 7),
    ("solid-to-bodies", 11),
    ("crumbling", 12),
    ("one-way", 14),
    ("one-way-behind", 15),
    ("spikes", 16),
];

/// The colours a picture draws pattern values 0-3 in.
const GREYS: [u8; 4] = [0x0F, 0x00, 0x10, 0x30];

#[derive(Deserialize)]
struct CustomFile {
    columns: u64,
    tilecount: u64,
    #[serde(default)]
    tiles: Vec<CustomTile>,
}

#[derive(Deserialize)]
struct CustomTile {
    id: u64,
    #[serde(default)]
    properties: Vec<Property>,
}

/// `custom.tsj`'s tiles by id (`None` where a tile has no definition), checked before anything
/// is read; `handles` are the animation handles a corner's bank may be.
pub(super) fn parse(text: &str, handles: &BTreeSet<u8>) -> Result<Vec<Option<Metatile>>, String> {
    let file: CustomFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if file.columns != 16 {
        return Err(format!("columns {}, not 16", file.columns));
    }
    let count = usize::try_from(file.tilecount)
        .ok()
        .filter(|&count| (1..=CLASS_COUNT).contains(&count))
        .ok_or_else(|| {
            format!(
                "tilecount {}, not 1 to the {CLASS_COUNT} a stage's metatiles hold",
                file.tilecount
            )
        })?;
    let mut metatiles = vec![None; count];
    for tile in &file.tiles {
        let slot = usize::try_from(tile.id)
            .ok()
            .and_then(|id| metatiles.get_mut(id))
            .ok_or_else(|| format!("tile {} is past tilecount {count}", tile.id))?;
        if slot.is_some() {
            return Err(format!("tile {} is defined twice", tile.id));
        }
        let metatile = metatile(&tile.properties, handles)
            .map_err(|reason| format!("tile {}: {reason}", tile.id))?;
        *slot = Some(metatile);
    }
    Ok(metatiles)
}

fn metatile(properties: &[Property], handles: &BTreeSet<u8>) -> Result<Metatile, String> {
    let value = |name: &str| {
        properties
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.value)
            .ok_or_else(|| format!("no `{name}`"))
    };
    let texts: Option<Vec<&str>> = value("tiles")?
        .as_array()
        .map(|items| items.iter().map(|item| item["value"].as_str()).collect())
        .and_then(|texts| texts);
    let texts: [&str; 4] = texts
        .and_then(|texts| texts.try_into().ok())
        .ok_or("`tiles` is not a list of four strings")?;
    let mut corners = [(0, 0); 4];
    for (corner, text) in corners.iter_mut().zip(texts) {
        *corner = corner_of(text, handles)?;
    }
    let palette = value("palette")?;
    let palette = palette
        .as_u64()
        .and_then(|palette| u8::try_from(palette).ok())
        .filter(|&palette| palette < 4)
        .ok_or_else(|| format!("`palette` {palette} is not 0-3"))?;
    let surface = value("surface")?.as_str().unwrap_or_default();
    let (_, class) = SURFACES
        .into_iter()
        .find(|&(name, _)| name == surface)
        .ok_or_else(|| format!("unknown `surface` `{surface}`"))?;
    Ok(Metatile {
        corners,
        palette,
        class,
    })
}

/// A corner's `"bb:tt"`: its bank and tile.
fn corner_of(text: &str, handles: &BTreeSet<u8>) -> Result<(u8, u8), String> {
    let hex = |digits: &str| {
        let lower_hex = |byte: u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte);
        u8::from_str_radix(digits, 16)
            .ok()
            .filter(|_| digits.len() == 2 && digits.bytes().all(lower_hex))
    };
    let (bank, tile) = text
        .split_once(':')
        .and_then(|(bank, tile)| Some((hex(bank)?, hex(tile)?)))
        .ok_or_else(|| format!("corner `{text}` is not `bb:tt` in lowercase hex"))?;
    if tile > 0x3f {
        return Err(format!("tile ${tile:02x} is past $3f"));
    }
    if bank & 0x80 != 0 && !handles.contains(&bank) {
        return Err(format!(
            "bank ${bank:02x} is not an animation handle the game's rooms load"
        ));
    }
    Ok((bank, tile))
}

/// The picture of `custom.tsj`'s `text`, RGBA, [`TILESET_PX`] wide and 16 px a row of 16
/// metatiles, each corner drawn from the bank the player's ROM loads for it in four greys;
/// clear where a tile has no definition.
///
/// # Errors
///
/// The ROM's levels do not import, or `text` is not a valid `custom.tsj`.
pub fn custom_picture(prg: &[u8], chr: &[u8], text: &str) -> Result<Vec<u8>, String> {
    let catalog = Catalog::from_rom(prg, chr).ok_or("the ROM's levels do not import")?;
    let metatiles = parse(text, &catalog.handles())?;
    let side = TILESET_PX as usize;
    let mut picture = vec![0; side * metatiles.len().div_ceil(16) * 16 * 4];
    for (id, metatile) in metatiles.iter().enumerate() {
        let Some(metatile) = metatile else {
            continue;
        };
        for (corner, &(bank, tile)) in metatile.corners.iter().enumerate() {
            let banks = [loaded_bank(prg, bank); 4];
            let pattern = chr::tile(chr, &banks, usize::from(tile) * chr::TILE_BYTES);
            draw_corner(&mut picture, id, corner, pattern, &GREYS);
        }
    }
    Ok(picture)
}

#[cfg(test)]
pub(super) mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::palette::rgba;

    /// A `custom.tsj` of `tilecount` tiles defining `tiles`: id, corners, palette and surface.
    pub(in crate::levels) fn tileset(
        tilecount: u64,
        tiles: &[(u64, [&str; 4], i64, &str)],
    ) -> String {
        let tiles: Vec<Value> = tiles
            .iter()
            .map(|&(id, corners, palette, surface)| tile(id, corners, palette, surface))
            .collect();
        file(16, tilecount, &tiles)
    }

    fn file(columns: u64, tilecount: u64, tiles: &[Value]) -> String {
        json!({
            "columns": columns,
            "image": "../../tilesets/packs/test/custom.png",
            "name": "custom",
            "tilecount": tilecount,
            "tileheight": 16,
            "tilewidth": 16,
            "tiles": tiles,
            "type": "tileset",
        })
        .to_string()
    }

    fn tile(id: u64, corners: [&str; 4], palette: i64, surface: &str) -> Value {
        let corners = corners.map(|corner| json!({ "type": "string", "value": corner }));
        json!({ "id": id, "properties": [
            { "name": "palette", "type": "int", "value": palette },
            { "name": "surface", "propertytype": "surface", "type": "string", "value": surface },
            { "name": "tiles", "type": "list", "value": corners },
        ]})
    }

    const GOOD: [&str; 4] = ["2c:05", "2c:06", "02:3f", "81:00"];

    #[test]
    fn a_good_custom_tileset_parses() {
        let text = tileset(4, &[(1, GOOD, 2, "open")]);
        let parsed = parse(&text, &BTreeSet::from([0x81])).expect("parses");
        let expected = Metatile {
            corners: [(0x2c, 5), (0x2c, 6), (2, 0x3f), (0x81, 0)],
            palette: 2,
            class: 7,
        };
        assert_eq!(parsed, [None, Some(expected), None, None]);
    }

    #[test]
    fn bad_custom_tilesets_are_refused() {
        let handles = BTreeSet::from([0x81]);
        let refused = |text: &str| parse(text, &handles).expect_err("refused");
        let with = |corners, palette, surface| tileset(4, &[(0, corners, palette, surface)]);
        let cases = [
            (
                tileset(209, &[]),
                "tilecount 209, not 1 to the 208 a stage's metatiles hold",
            ),
            (
                tileset(u64::MAX, &[]),
                "tilecount 18446744073709551615, not 1 to the 208 a stage's metatiles hold",
            ),
            (
                tileset(0, &[]),
                "tilecount 0, not 1 to the 208 a stage's metatiles hold",
            ),
            (
                tileset(4, &[(0, GOOD, 0, "open")]).replacen(
                    r#""value":[{"#,
                    r#""value":[{"type":"int","value":1},{"#,
                    1,
                ),
                "tile 0: `tiles` is not a list of four strings",
            ),
            (file(8, 4, &[]), "columns 8, not 16"),
            (
                with(["2C:05", "2c:06", "02:3f", "81:00"], 0, "open"),
                "tile 0: corner `2C:05` is not `bb:tt` in lowercase hex",
            ),
            (
                with(["2c:40", "2c:06", "02:3f", "81:00"], 0, "open"),
                "tile 0: tile $40 is past $3f",
            ),
            (
                with(["85:00", "2c:06", "02:3f", "81:00"], 0, "open"),
                "tile 0: bank $85 is not an animation handle the game's rooms load",
            ),
            (with(GOOD, 4, "open"), "tile 0: `palette` 4 is not 0-3"),
            (with(GOOD, 0, "lava"), "tile 0: unknown `surface` `lava`"),
            (
                tileset(4, &[(3, GOOD, 0, "open"), (3, GOOD, 0, "open")]),
                "tile 3 is defined twice",
            ),
            (
                tileset(4, &[(9, GOOD, 0, "open")]),
                "tile 9 is past tilecount 4",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(refused(&text), expected);
        }
    }

    #[test]
    fn custom_picture_draws_the_corners_from_chr() {
        let Some(rom) = rotj_rom::for_test() else {
            return;
        };
        let corners = ["2c:05", "2c:06", "02:3f", "03:00"];
        let text = tileset(1, &[(0, corners, 0, "solid")]);
        let picture = custom_picture(&rom.prg, &rom.chr, &text).expect("draws");
        let side = TILESET_PX as usize;
        assert_eq!(picture.len(), side * 16 * 4);
        for (corner, (bank, tile)) in [(0x2c_u8, 5_u8), (0x2c, 6), (2, 0x3f), (3, 0)]
            .into_iter()
            .enumerate()
        {
            let banks = [loaded_bank(&rom.prg, bank); 4];
            let pattern = chr::tile(&rom.chr, &banks, usize::from(tile) * chr::TILE_BYTES);
            for y in 0..8 {
                for x in 0..8 {
                    let at = ((corner / 2 * 8 + y) * side + corner % 2 * 8 + x) * 4;
                    let colour = GREYS[usize::from(chr::pixel(pattern, x, y))];
                    assert_eq!(
                        picture[at..][..4],
                        rgba(colour),
                        "corner {corner} ({x}, {y})"
                    );
                }
            }
        }
        assert!(
            picture[16 * 4..side * 4].iter().all(|&byte| byte == 0),
            "past tile 0 is clear"
        );
    }
}
