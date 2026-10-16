//! NES master palette: the PPU's 64 colors as RGB, for previews and the
//! engine's video output.
//!
//! The NES outputs composite video, so no RGB table is "the" truth. This is
//! FirebrandX's Composite Direct palette, measured from a real NES's composite
//! output (FCEUX's `output/palettes/Composite_Direct_FBX.pal`), kept byte for byte
//! at `assets/graphics/palette-sources/Composite_Direct_FBX.pal`. Until 2026-10-15
//! the port used Nestopia's RGB table, which is an RGB PPU's (PlayChoice, Vs.
//! System) look, not a NES's.

/// RGB for each of the PPU's 64 colors, indexed by palette-RAM value `0..=63`.
pub const NES_PALETTE: Rgb = [
    [0x65, 0x65, 0x65],
    [0x00, 0x12, 0x7d],
    [0x18, 0x00, 0x8e],
    [0x36, 0x00, 0x82],
    [0x56, 0x00, 0x5d],
    [0x5a, 0x00, 0x18],
    [0x4f, 0x05, 0x00],
    [0x38, 0x19, 0x00],
    [0x1d, 0x31, 0x00],
    [0x00, 0x3d, 0x00],
    [0x00, 0x41, 0x00],
    [0x00, 0x3b, 0x17],
    [0x00, 0x2e, 0x55],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xaf, 0xaf, 0xaf],
    [0x19, 0x4e, 0xc8],
    [0x47, 0x2f, 0xe3],
    [0x6b, 0x1f, 0xd7],
    [0x93, 0x1b, 0xae],
    [0x9e, 0x1a, 0x5e],
    [0x99, 0x32, 0x00],
    [0x7b, 0x4b, 0x00],
    [0x5b, 0x67, 0x00],
    [0x26, 0x7a, 0x00],
    [0x00, 0x82, 0x00],
    [0x00, 0x7a, 0x3e],
    [0x00, 0x6e, 0x8a],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xff, 0xff, 0xff],
    [0x64, 0xa9, 0xff],
    [0x8e, 0x89, 0xff],
    [0xb6, 0x76, 0xff],
    [0xe0, 0x6f, 0xff],
    [0xef, 0x6c, 0xc4],
    [0xf0, 0x80, 0x6a],
    [0xd8, 0x98, 0x2c],
    [0xb9, 0xb4, 0x0a],
    [0x83, 0xcb, 0x0c],
    [0x5b, 0xd6, 0x3f],
    [0x4a, 0xd1, 0x7e],
    [0x4d, 0xc7, 0xcb],
    [0x4c, 0x4c, 0x4c],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xff, 0xff, 0xff],
    [0xc7, 0xe5, 0xff],
    [0xd9, 0xd9, 0xff],
    [0xe9, 0xd1, 0xff],
    [0xf9, 0xce, 0xff],
    [0xff, 0xcc, 0xf1],
    [0xff, 0xd4, 0xcb],
    [0xf8, 0xdf, 0xb1],
    [0xed, 0xea, 0xa4],
    [0xd6, 0xf4, 0xa4],
    [0xc5, 0xf8, 0xb8],
    [0xbe, 0xf6, 0xd3],
    [0xbf, 0xf1, 0xf1],
    [0xb9, 0xb9, 0xb9],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
];

/// A 64-colour table, indexed by palette-RAM value `0..=63`.
pub type Rgb = [[u8; 3]; 64];

/// The 2C03 RGB PPU (PlayChoice, Vs. System): each channel a 3-bit DAC level
/// (<https://www.nesdev.org/wiki/PPU_palettes>), written as `255 * level / 7`.
pub const RGB_PPU: Rgb = rgb_ppu([
    0o333, 0o014, 0o006, 0o326, 0o403, 0o503, 0o510, 0o420, 0o320, 0o120, 0o031, 0o040, 0o022,
    0o000, 0o000, 0o000, 0o555, 0o036, 0o027, 0o407, 0o507, 0o704, 0o700, 0o630, 0o430, 0o140,
    0o040, 0o053, 0o044, 0o000, 0o000, 0o000, 0o777, 0o357, 0o447, 0o637, 0o707, 0o737, 0o740,
    0o750, 0o660, 0o360, 0o070, 0o276, 0o077, 0o000, 0o000, 0o000, 0o777, 0o567, 0o657, 0o757,
    0o747, 0o755, 0o764, 0o772, 0o773, 0o572, 0o473, 0o276, 0o467, 0o000, 0o000, 0o000,
]);

const fn rgb_ppu(levels: [u16; 64]) -> Rgb {
    const BYTES: [u8; 8] = [0x00, 0x24, 0x49, 0x6D, 0x92, 0xB6, 0xDB, 0xFF];
    let mut table = [[0; 3]; 64];
    let mut i = 0;
    while i < 64 {
        let level = levels[i];
        table[i] = [
            BYTES[(level >> 6) as usize & 7],
            BYTES[(level >> 3) as usize & 7],
            BYTES[level as usize & 7],
        ];
        i += 1;
    }
    table
}

/// The 2C02's composite signal decoded to RGB, after <https://www.nesdev.org/wiki/NTSC_video>:
/// each colour's square wave over 12 phases at the terminated voltages, demodulated to YUV.
#[must_use]
pub fn ntsc() -> Rgb {
    const LOW: [f64; 4] = [0.228, 0.312, 0.552, 0.880];
    const HIGH: [f64; 4] = [0.616, 0.840, 1.100, 1.100];
    const BLACK: f64 = 0.312;
    const WHITE: f64 = 1.100;
    let byte = |x: f64| {
        (0..=255u8)
            .find(|&b| f64::from(b) + 0.5 > 255.0 * x)
            .unwrap_or(255)
    };
    std::array::from_fn(|i| {
        let hue = i & 15;
        let level = if hue >= 14 { 1 } else { (i >> 4) & 3 };
        let (low, high) = match hue {
            0 => (HIGH[level], HIGH[level]),
            13.. => (LOW[level], LOW[level]),
            _ => (LOW[level], HIGH[level]),
        };
        let (mut y, mut u, mut v) = (0.0, 0.0, 0.0);
        for phase in 0..12u8 {
            let signal = if (hue + usize::from(phase)) % 12 < 6 {
                high
            } else {
                low
            };
            let level = (signal - BLACK) / (WHITE - BLACK) / 12.0;
            let angle = std::f64::consts::PI * (f64::from(phase) + 2.5) / 6.0;
            y += level;
            u += level * angle.sin() * 2.0;
            v += level * angle.cos() * 2.0;
        }
        [
            byte(y + 1.139_883 * v),
            byte(y - 0.394_642 * u - 0.580_622 * v),
            byte(y + 2.032_062 * u),
        ]
    })
}

/// A `.pal` file's colours: 192 bytes, or 1536 with the emphasis variants after them.
#[must_use]
pub fn from_pal(bytes: &[u8]) -> Option<Rgb> {
    if bytes.len() != 192 && bytes.len() != 1536 {
        return None;
    }
    Some(std::array::from_fn(|i| {
        [bytes[3 * i], bytes[3 * i + 1], bytes[3 * i + 2]]
    }))
}

/// Opaque RGBA for a palette-RAM value; indices above `$3F` wrap, as on the PPU.
#[must_use]
pub fn rgba(index: u8) -> [u8; 4] {
    let [r, g, b] = NES_PALETTE[usize::from(index & 0x3f)];
    [r, g, b, 0xff]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_source_palette() {
        let source =
            include_bytes!("../../../assets/graphics/palette-sources/Composite_Direct_FBX.pal");
        assert_eq!(NES_PALETTE.as_flattened(), source);
    }

    #[test]
    fn rgb_ppu_is_the_2c03_levels() {
        const LEVELS: [u8; 8] = [0x00, 0x24, 0x49, 0x6D, 0x92, 0xB6, 0xDB, 0xFF];
        assert!(RGB_PPU.as_flattened().iter().all(|c| LEVELS.contains(c)));
        assert_eq!(RGB_PPU[0x30], [0xFF; 3]);
        assert_eq!(RGB_PPU[0x0F], [0; 3]);
    }

    #[test]
    fn ntsc_greys_are_neutral_and_ordered() {
        let table = ntsc();
        let greys = [0x00, 0x10, 0x20, 0x30].map(|i| table[i]);
        for [r, g, b] in greys {
            assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "{greys:?}");
        }
        assert!(
            greys.windows(2).all(|pair| pair[0][1] <= pair[1][1]),
            "{greys:?}"
        );
        assert!(
            greys[0][1] < greys[1][1] && greys[1][1] < greys[2][1],
            "{greys:?}"
        );
        assert_eq!(table[0x0F], [0, 0, 0]);
        let sum = |[r, g, b]: [u8; 3]| u16::from(r) + u16::from(g) + u16::from(b);
        assert!(table.iter().all(|&c| sum(c) <= sum(table[0x30])));
    }

    #[test]
    fn ntsc_is_pinned() {
        assert_eq!(
            crate::digest::fnv1a(ntsc().as_flattened()),
            5_210_628_301_385_109_089
        );
    }

    #[test]
    fn pal_files_are_192_or_1536_bytes() {
        assert!(from_pal(&[0; 192]).is_some());
        let mut emphasis = vec![0x11; 192];
        emphasis.resize(1536, 0x22);
        assert_eq!(from_pal(&emphasis), Some([[0x11; 3]; 64]));
        for len in [0, 191, 193, 1535] {
            assert_eq!(from_pal(&vec![0; len]), None, "{len} bytes");
        }
    }

    #[test]
    fn rgba_is_opaque_and_wraps_above_3f() {
        assert_eq!(rgba(0x20), [0xff, 0xff, 0xff, 0xff]);
        assert_eq!(rgba(0x60), rgba(0x20));
    }
}
