//! MD5 (RFC 1321), only to identify the ROM dump: it identifies, it does not authenticate.

use std::fmt::Write as _;

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor(abs(sin(i + 1)) * 2^32)` (RFC 1321 section 3.4).
const K: [u32; 64] = [
    0xD76A_A478,
    0xE8C7_B756,
    0x2420_70DB,
    0xC1BD_CEEE,
    0xF57C_0FAF,
    0x4787_C62A,
    0xA830_4613,
    0xFD46_9501,
    0x6980_98D8,
    0x8B44_F7AF,
    0xFFFF_5BB1,
    0x895C_D7BE,
    0x6B90_1122,
    0xFD98_7193,
    0xA679_438E,
    0x49B4_0821,
    0xF61E_2562,
    0xC040_B340,
    0x265E_5A51,
    0xE9B6_C7AA,
    0xD62F_105D,
    0x0244_1453,
    0xD8A1_E681,
    0xE7D3_FBC8,
    0x21E1_CDE6,
    0xC337_07D6,
    0xF4D5_0D87,
    0x455A_14ED,
    0xA9E3_E905,
    0xFCEF_A3F8,
    0x676F_02D9,
    0x8D2A_4C8A,
    0xFFFA_3942,
    0x8771_F681,
    0x6D9D_6122,
    0xFDE5_380C,
    0xA4BE_EA44,
    0x4BDE_CFA9,
    0xF6BB_4B60,
    0xBEBF_BC70,
    0x289B_7EC6,
    0xEAA1_27FA,
    0xD4EF_3085,
    0x0488_1D05,
    0xD9D4_D039,
    0xE6DB_99E5,
    0x1FA2_7CF8,
    0xC4AC_5665,
    0xF429_2244,
    0x432A_FF97,
    0xAB94_23A7,
    0xFC93_A039,
    0x655B_59C3,
    0x8F0C_CC92,
    0xFFEF_F47D,
    0x8584_5DD1,
    0x6FA8_7E4F,
    0xFE2C_E6E0,
    0xA301_4314,
    0x4E08_11A1,
    0xF753_7E82,
    0xBD3A_F235,
    0x2AD7_D2BB,
    0xEB86_D391,
];

/// The MD5 digest of `bytes` as 32 lowercase hex digits.
#[must_use]
pub fn md5_hex(bytes: &[u8]) -> String {
    let mut state: [u32; 4] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476];
    let mut message = bytes.to_vec();
    let bits = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_le_bytes());
    for block in message.as_chunks::<64>().0 {
        let words: Vec<u32> = block
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (mixed, index) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a
                .wrapping_add(mixed)
                .wrapping_add(K[i])
                .wrapping_add(words[index])
                .rotate_left(S[i]);
            (a, d, c) = (d, c, b);
            b = b.wrapping_add(rotated);
        }
        for (word, add) in state.iter_mut().zip([a, b, c, d]) {
            *word = word.wrapping_add(add);
        }
    }
    state
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::md5_hex;

    #[test]
    fn md5_matches_rfc_1321() {
        for (input, digest) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(md5_hex(input.as_bytes()), digest, "{input:?}");
        }
    }
}
