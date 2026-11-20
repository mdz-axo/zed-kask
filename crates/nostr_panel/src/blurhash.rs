//! A blurhash decoder, mirroring the reference implementation (the
//! `blurhash` crate 0.2.3, a port of woltapp/blurhash). A blurhash is a
//! compact stand-in for an image; a note carries one in its own `imeta`
//! tag (NIP-92), so decoding it fetches nothing — the viewer's third
//! rule holds. Decode-only: the panel never encodes.

use image::{Rgba, RgbaImage};

/// The base83 alphabet, in index order.
const BASE83: &[u8; 83] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~";

/// One base83 character's value, or `None` for a character outside the
/// alphabet.
fn base83_digit(byte: u8) -> Option<u8> {
    BASE83
        .iter()
        .position(|&known| known == byte)
        .map(|index| index as u8)
}

/// The value of a base83 string, or `None` if any character is outside
/// the alphabet.
fn decode83(text: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    for byte in text {
        value = value * 83 + base83_digit(*byte)? as u64;
    }
    Some(value)
}

/// sRGB 0-255 to linear 0.0-1.0, the standard sRGB transfer function.
fn srgb_to_linear(value: u8) -> f32 {
    let v = value as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear 0.0-1.0 to sRGB 0-255. The reference's original truncating
/// form: the crate's distributed rounding turns a pure white into 256
/// and then into 0, so the truncation is kept.
fn linear_to_srgb(value: f32) -> u8 {
    let v = value.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        (v * 12.92 * 255.0 + 0.5) as u8
    } else {
        ((1.055 * v.powf(1.0 / 2.4) - 0.055) * 255.0 + 0.5) as u8
    }
}

/// Sign-preserving exponentiation, the AC components' shape.
fn sign_pow(value: f32, exponent: f32) -> f32 {
    value.abs().powf(exponent).copysign(value)
}

/// One AC component: three 0-18 quantised channels, each centred on 9
/// and sign-squared, scaled by the hash's stated maximum.
fn ac(value: u32, maximum_value: f32) -> [f32; 3] {
    let quant_r = (value / (19 * 19)) as i32;
    let quant_g = ((value / 19) % 19) as i32;
    let quant_b = (value % 19) as i32;
    [
        sign_pow((quant_r - 9) as f32 / 9.0, 2.0) * maximum_value,
        sign_pow((quant_g - 9) as f32 / 9.0, 2.0) * maximum_value,
        sign_pow((quant_b - 9) as f32 / 9.0, 2.0) * maximum_value,
    ]
}

/// Decode a blurhash to an RGBA image of the given size. `None` when the
/// text is not a blurhash: a character outside the alphabet, or a length
/// that does not match its own component count.
pub(crate) fn decode(hash: &str, width: u32, height: u32) -> Option<RgbaImage> {
    let bytes = hash.as_bytes();
    if bytes.len() < 6 || !hash.is_ascii() {
        return None;
    }
    let size_flag = decode83(&bytes[0..1])? as usize;
    let num_y = size_flag / 9 + 1;
    let num_x = size_flag % 9 + 1;
    if bytes.len() != 4 + 2 * num_x * num_y {
        return None;
    }
    let maximum_value = (decode83(&bytes[1..2])? as f32 + 1.0) / 166.0;

    // The components: the DC colour first, then one AC per pair.
    let mut colors: Vec<[f32; 3]> = Vec::with_capacity(num_x * num_y);
    let dc = decode83(&bytes[2..6])? as u32;
    colors.push([
        srgb_to_linear(((dc >> 16) & 255) as u8),
        srgb_to_linear(((dc >> 8) & 255) as u8),
        srgb_to_linear((dc & 255) as u8),
    ]);
    for index in 1..num_x * num_y {
        let value = decode83(&bytes[4 + index * 2..6 + index * 2])? as u32;
        colors.push(ac(value, maximum_value));
    }

    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let mut pixel = colors[0];
            for j in 0..num_y {
                for i in 0..num_x {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let basis = (std::f32::consts::PI * x as f32 * i as f32 / width as f32).cos()
                        * (std::f32::consts::PI * y as f32 * j as f32 / height as f32).cos();
                    let color = colors[j * num_x + i];
                    pixel[0] += color[0] * basis;
                    pixel[1] += color[1] * basis;
                    pixel[2] += color[2] * basis;
                }
            }
            image.put_pixel(
                x,
                y,
                Rgba([
                    linear_to_srgb(pixel[0]),
                    linear_to_srgb(pixel[1]),
                    linear_to_srgb(pixel[2]),
                    255,
                ]),
            );
        }
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: the decoder reads the reference's own shapes — base83, the
    /// component header, the DC colour, a zero AC — and refuses a text
    /// whose length does not match its own component count or whose
    /// characters are not the alphabet.
    #[test]
    fn decodes_the_reference_shapes() {
        // The reference crate's own vector: encode83(6869, 2) == "~$".
        assert_eq!(decode83(b"~$"), Some(6869));
        assert_eq!(decode83(b"\x80"), None);

        // A 1x1 white hash: size flag 0, maximum flag 0, DC 0xFFFFFF.
        let white = decode("00TSUA", 3, 2).expect("a 1x1 hash decodes");
        for pixel in white.pixels() {
            assert_eq!(pixel, &Rgba([255, 255, 255, 255]));
        }

        // A 1x1 black hash: DC 0 (base83's zero digit is '0').
        let black = decode("000000", 2, 2).expect("a 1x1 hash decodes");
        for pixel in black.pixels() {
            assert_eq!(pixel, &Rgba([0, 0, 0, 255]));
        }

        // A 2x1 hash whose AC quantises to zero: still white everywhere.
        let flat = decode("10TSUAfQ", 4, 1).expect("a 2x1 hash decodes");
        for pixel in flat.pixels() {
            assert_eq!(pixel, &Rgba([255, 255, 255, 255]));
        }

        // The reference's example hash parses as 4x3 and decodes in
        // range.
        let photo = decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 8, 6).expect("the example decodes");
        assert_eq!(photo.dimensions(), (8, 6));
        for pixel in photo.pixels() {
            assert_eq!(pixel[3], 255);
        }

        // A length that does not match the component count, and a
        // character outside the alphabet, are not blurhashes.
        assert!(decode("00TSU", 2, 2).is_none());
        assert!(
            decode("00TSU/", 2, 2).is_none(),
            "'/' is not in the alphabet"
        );
    }
}
