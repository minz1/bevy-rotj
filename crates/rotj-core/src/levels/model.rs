//! The subset of Tiled 1.12's JSON formats a pack reads and writes, and `pack.json`. Every
//! struct keeps the fields it does not know in `extra`, so a load and save keeps them.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map as JsonMap, Number, Value};

/// The JSON fields a struct does not know.
pub type Extra = JsonMap<String, Value>;

/// A Tiled map (`.tmj`), its keys in Tiled's order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Map {
    /// Rows of tiles.
    pub height: u32,
    /// Always false in a pack.
    pub infinite: bool,
    /// The layers, bottom first.
    pub layers: Vec<Layer>,
    /// The next free layer id.
    pub nextlayerid: u32,
    /// The next free object id.
    pub nextobjectid: u32,
    /// `"orthogonal"`.
    pub orientation: String,
    /// The map's custom properties.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "sorted"
    )]
    pub properties: Vec<Property>,
    /// `"right-down"`.
    pub renderorder: String,
    /// The Tiled version that saved the file.
    pub tiledversion: String,
    /// A tile's height in px.
    pub tileheight: u32,
    /// The tilesets the layers' gids name.
    pub tilesets: Vec<TilesetRef>,
    /// A tile's width in px.
    pub tilewidth: u32,
    /// `"map"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The JSON format version.
    pub version: String,
    /// Columns of tiles.
    pub width: u32,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A map's reference to an external tileset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TilesetRef {
    /// The gid of the tileset's first tile.
    pub firstgid: u32,
    /// The tileset file, relative to the map.
    pub source: String,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A tile layer or an object group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// A tile layer's gids, row by row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u32>>,
    /// An object group's draw order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draworder: Option<String>,
    /// A tile layer's rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The layer's id in the map.
    pub id: u32,
    /// The layer's name.
    pub name: String,
    /// An object group's objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<Object>>,
    /// The layer's opacity, 0-1.
    pub opacity: Number,
    /// `"tilelayer"` or `"objectgroup"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Whether the editor shows the layer.
    pub visible: bool,
    /// A tile layer's columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// The layer's x offset in tiles.
    pub x: i32,
    /// The layer's y offset in tiles.
    pub y: i32,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// An object in an object group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    /// Its height in px.
    pub height: Number,
    /// Its id, unique in the map.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// Whether it is a point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<bool>,
    /// Its custom properties.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "sorted"
    )]
    pub properties: Vec<Property>,
    /// Its rotation in degrees.
    pub rotation: Number,
    /// Its class.
    #[serde(rename = "type")]
    pub kind: String,
    /// Whether the editor shows it.
    pub visible: bool,
    /// Its width in px.
    pub width: Number,
    /// Its x.
    pub x: Position,
    /// Its y.
    pub y: Position,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A custom property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// Its name.
    pub name: String,
    /// The project file's enum or class an `enum`-valued `string` names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propertytype: Option<String>,
    /// `int`, `bool`, `string` or `list`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Its value; a `list`'s is an array of `{"type", "value"}` items.
    pub value: Value,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Property {
    /// An `int` property.
    #[must_use]
    pub fn int(name: &str, value: impl Into<i64>) -> Property {
        Property::new(name, "int", None, Value::from(value.into()))
    }

    /// A `string` property holding a value of the project file's enum `propertytype`.
    #[must_use]
    pub fn enumerated(name: &str, propertytype: &str, value: &str) -> Property {
        Property::new(name, "string", Some(propertytype), Value::from(value))
    }

    /// A `list` property of `int` items.
    #[must_use]
    pub fn int_list(name: &str, values: impl IntoIterator<Item = i64>) -> Property {
        let items = values
            .into_iter()
            .map(|value| serde_json::json!({ "type": "int", "value": value }))
            .collect();
        Property::new(name, "list", None, Value::Array(items))
    }

    fn new(name: &str, kind: &str, propertytype: Option<&str>, value: Value) -> Property {
        Property {
            name: name.to_owned(),
            propertytype: propertytype.map(str::to_owned),
            kind: kind.to_owned(),
            value,
            extra: Extra::new(),
        }
    }
}

/// Serialises properties sorted by name, as Tiled writes them.
fn sorted<S: Serializer>(properties: &[Property], serializer: S) -> Result<S::Ok, S::Error> {
    let mut by_name: Vec<&Property> = properties.iter().collect();
    by_name.sort_by(|a, b| a.name.cmp(&b.name));
    by_name.serialize(serializer)
}

/// A level coordinate in 1/16 px, written in px: an integer when whole, else the exact
/// decimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(pub u16);

impl Position {
    /// The position a JSON number of px names, if it is a whole number of sixteenths in range.
    #[must_use]
    pub fn from_px(px: &Number) -> Option<Position> {
        let sixteenths = px.as_f64()? * 16.0;
        if sixteenths.fract() != 0.0 || !(0.0..=65535.0).contains(&sixteenths) {
            return None;
        }
        // f64's Display writes a whole value without a fraction, so this is the exact integer.
        sixteenths.to_string().parse().ok().map(Position)
    }
}

impl Serialize for Position {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.is_multiple_of(16) {
            serializer.serialize_u16(self.0 / 16)
        } else {
            serializer.serialize_f64(f64::from(self.0) / 16.0)
        }
    }
}

impl<'de> Deserialize<'de> for Position {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Position, D::Error> {
        let px = Number::deserialize(deserializer)?;
        Position::from_px(&px).ok_or_else(|| {
            D::Error::custom(format!("{px} px is not a whole number of 1/16 px in range"))
        })
    }
}

/// A Tiled world file (`.world`): the rooms' layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct World {
    /// The maps and where they sit.
    pub maps: Vec<WorldMap>,
    /// Whether Tiled shows only the maps beside the open one.
    #[serde(rename = "onlyShowAdjacentMaps")]
    pub only_show_adjacent_maps: bool,
    /// `"world"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A map in a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldMap {
    /// The map file, relative to the world.
    #[serde(rename = "fileName")]
    pub file_name: String,
    /// Its height in px.
    pub height: u32,
    /// Its width in px.
    pub width: u32,
    /// Its x in px.
    pub x: i64,
    /// Its y in px.
    pub y: i64,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `pack.json`: the pack's identity and its stages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackFile {
    /// The format version, 1.
    pub format: u32,
    /// The pack's name.
    pub name: String,
    /// Who made it.
    pub author: String,
    /// `classic` or `modern`.
    pub engine: String,
    /// The stages in play order.
    pub stages: Vec<PackStage>,
    /// The password screen's passwords the pause never shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_passwords: Vec<ExtraPassword>,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A stage in `pack.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackStage {
    /// The stage's name.
    pub name: String,
    /// Its rooms' files in play order.
    pub rooms: Vec<String>,
    /// The area's 16 physical pages in order, each the room regions that show it.
    pub pages: Vec<Vec<Region>>,
    /// The HUD's stage label, `d-d`.
    pub hud_label: String,
    /// The song it starts, `song-xx`, or `keep`.
    pub music: String,
    /// The picture shown before its banner, `picture-xx`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutscene: Option<String>,
    /// The checkpoint's password, four letters `A`-`Z`; a stage with none is no checkpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// What clearing it does: `next-stage` or `ending`.
    pub on_clear: String,
    /// Fields (and knobs) this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A password the pause never shows, in `pack.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtraPassword {
    /// Four letters `A`-`Z`.
    pub password: String,
    /// The name of the checkpoint it starts.
    pub stage: String,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A room's page-table entry (0-31), or an extra page (32 and up).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// The room's file.
    pub room: String,
    /// The entry.
    pub entry: u32,
    /// Fields this model does not know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `value` written as a pack writes its files: pretty JSON and a final newline.
#[must_use]
pub fn to_text<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("the model serialises");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(text: &str) -> Option<Position> {
        Position::from_px(&serde_json::from_str(text).expect("a number"))
    }

    #[test]
    fn positions_read_every_number_form_of_a_sixteenth() {
        assert_eq!(px("29"), Some(Position(29 * 16)));
        assert_eq!(px("29.0"), Some(Position(29 * 16)));
        assert_eq!(px("29.9375"), Some(Position(29 * 16 + 15)));
        assert_eq!(px("0.5"), Some(Position(8)));
        assert_eq!(px("4095.9375"), Some(Position(u16::MAX)));
        assert_eq!(px("4096"), None);
        assert_eq!(px("0.1"), None);
        assert_eq!(px("-1"), None);
        assert_eq!(px("1e2"), Some(Position(1600)));
    }

    #[test]
    fn positions_write_whole_px_as_integers_and_the_rest_exactly() {
        for (position, text) in [(29 * 16, "29"), (29 * 16 + 15, "29.9375"), (8, "0.5")] {
            assert_eq!(
                serde_json::to_string(&Position(position)).expect("serialises"),
                text
            );
        }
        for sixteenths in 0..=u16::MAX {
            let text = serde_json::to_string(&Position(sixteenths)).expect("serialises");
            assert_eq!(px(&text), Some(Position(sixteenths)), "{text}");
        }
    }

    #[test]
    fn properties_are_written_sorted_by_name() {
        let object = Object {
            height: 0.into(),
            id: 1,
            name: String::new(),
            point: Some(true),
            properties: vec![Property::int("state", 1), Property::int("flags", 2)],
            rotation: 0.into(),
            kind: "stomper".into(),
            visible: true,
            width: 0.into(),
            x: Position(0),
            y: Position(0),
            extra: Extra::new(),
        };
        let text = serde_json::to_string(&object).expect("serialises");
        assert!(text.find("\"flags\"") < text.find("\"state\""), "{text}");
    }
}
