//! Generated initials badges for tray and menu icons.
//!
//! Epic has no local avatar cache (unlike Steam's `avatarcache`), so each
//! account gets a deterministic colored rounded square with its initial:
//! the color is hashed from the account ID, the glyph comes from a tiny
//! embedded 5×7 bitmap font (no font dependency).

use image::{Rgba, RgbaImage};

/// Pleasant, white-glyph-friendly badge colors (picked by account-ID hash).
const PALETTE: [[u8; 3]; 9] = [
    [0x3B, 0x82, 0xF6], // blue
    [0x8B, 0x5C, 0xF6], // violet
    [0xEF, 0x44, 0x44], // red
    [0xEA, 0x58, 0x0C], // orange
    [0x10, 0xB9, 0x81], // emerald
    [0x0E, 0xA5, 0xE9], // sky
    [0xEC, 0x48, 0x99], // pink
    [0x14, 0xB8, 0xA6], // teal
    [0x64, 0x74, 0x8B], // slate
];

/// The character shown on the badge: the first glyph-mappable character of
/// the display name, else of the account ID, else 'E'. Common accented Latin
/// letters are transliterated (Ü -> U, é -> E, …) because the embedded font
/// only covers A-Z/0-9 — without this, umlaut names would render the '?'
/// fallback glyph.
pub fn initial_for(display_name: &str, account_id: &str) -> char {
    display_name
        .chars()
        .find_map(mappable_initial)
        .or_else(|| account_id.chars().find_map(mappable_initial))
        .unwrap_or('E')
}

/// Map a character to a glyph the embedded font can draw, if possible.
fn mappable_initial(c: char) -> Option<char> {
    if c.is_ascii_alphanumeric() {
        return Some(c.to_ascii_uppercase());
    }
    match c {
        'Ä' | 'ä' | 'À' | 'à' | 'Á' | 'á' | 'Â' | 'â' | 'Ã' | 'ã' | 'Å' | 'å' => Some('A'),
        'É' | 'é' | 'È' | 'è' | 'Ê' | 'ê' | 'Ë' | 'ë' => Some('E'),
        'Í' | 'í' | 'Ì' | 'ì' | 'Î' | 'î' | 'Ï' | 'ï' => Some('I'),
        'Ö' | 'ö' | 'Ò' | 'ò' | 'Ó' | 'ó' | 'Ô' | 'ô' | 'Õ' | 'õ' | 'Ø' | 'ø' => Some('O'),
        'Ü' | 'ü' | 'Ù' | 'ù' | 'Ú' | 'ú' | 'Û' | 'û' => Some('U'),
        'Ç' | 'ç' => Some('C'),
        'Ñ' | 'ñ' => Some('N'),
        'Ý' | 'ý' | 'Ÿ' | 'ÿ' => Some('Y'),
        'ß' => Some('S'),
        _ => None,
    }
}

/// Render a `size`×`size` RGBA initials badge. Returns `(rgba_bytes, size)`.
pub fn badge_rgba(account_id: &str, initial: char, size: u32) -> (Vec<u8>, u32) {
    let [r, g, b] = PALETTE[(fnv1a(account_id) as usize) % PALETTE.len()];
    let mut img = RgbaImage::from_pixel(size, size, Rgba([r, g, b, 255]));

    draw_glyph(&mut img, initial, size);
    apply_rounded_mask(&mut img, size);
    (img.into_raw(), size)
}

/// Draw the 5×7 glyph centered, scaled to roughly 60% of the badge height.
fn draw_glyph(img: &mut RgbaImage, ch: char, size: u32) {
    let glyph = glyph_for(ch);
    let scale = ((size * 3 / 5) / 7).max(1);
    let width = 5 * scale;
    let height = 7 * scale;
    let x0 = (size.saturating_sub(width)) / 2;
    let y0 = (size.saturating_sub(height)) / 2;

    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (0b10000 >> col) == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let x = x0 + col * scale + dx;
                    let y = y0 + row as u32 * scale + dy;
                    if x < size && y < size {
                        img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                    }
                }
            }
        }
    }
}

/// Apply an anti-aliased rounded-rectangle (rounded square) alpha mask in
/// place, so the badge renders with softly rounded corners. (Same shaping as
/// the Steam app's avatar icons.)
fn apply_rounded_mask(img: &mut RgbaImage, size: u32) {
    let half = size as f32 / 2.0;
    let center = half - 0.5;
    // Corner radius as a fraction of the icon size.
    let radius = (size as f32 * 0.28).max(2.0);
    for y in 0..size {
        for x in 0..size {
            // Signed distance to a rounded rectangle that fills the icon.
            let qx = (x as f32 - center).abs() - half + radius;
            let qy = (y as f32 - center).abs() - half + radius;
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let inside = qx.max(qy).min(0.0);
            let sdf = outside + inside - radius;
            // 1px anti-aliased edge: inside -> opaque, outside -> transparent.
            let factor = (0.5 - sdf).clamp(0.0, 1.0);
            let px = img.get_pixel_mut(x, y);
            px[3] = (px[3] as f32 * factor) as u8;
        }
    }
}

fn fnv1a(input: &str) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in input.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 5×7 bitmap glyphs, one `u8` per row, low 5 bits used (MSB = leftmost).
fn glyph_for(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        _ => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_has_expected_dimensions_and_content() {
        let (rgba, size) = badge_rgba("abc123", 'B', 32);
        assert_eq!(size, 32);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Corners are masked transparent, center is opaque.
        assert_eq!(rgba[3], 0, "top-left corner must be transparent");
        let center = ((16 * 32 + 16) * 4 + 3) as usize;
        assert_eq!(rgba[center], 255, "center must be opaque");
        // The glyph paints at least some white pixels.
        assert!(
            rgba.chunks_exact(4).any(|px| px == [255, 255, 255, 255]),
            "expected white glyph pixels"
        );
    }

    #[test]
    fn badge_color_is_deterministic_per_account() {
        let (a1, _) = badge_rgba("account-one", 'A', 16);
        let (a2, _) = badge_rgba("account-one", 'A', 16);
        assert_eq!(a1, a2);
    }

    #[test]
    fn initial_prefers_display_name() {
        assert_eq!(initial_for("benny", "12ab"), 'B');
        assert_eq!(initial_for("  •weird", "12ab"), 'W');
        assert_eq!(initial_for("---", "12ab"), '1');
        assert_eq!(initial_for("", ""), 'E');
    }

    #[test]
    fn accented_initials_are_transliterated() {
        assert_eq!(initial_for("Ümläut", "12ab"), 'U');
        assert_eq!(initial_for("Ödipus", "12ab"), 'O');
        assert_eq!(initial_for("Ärger", "12ab"), 'A');
        assert_eq!(initial_for("électro", "12ab"), 'E');
        // Unmappable scripts fall through to the account id.
        assert_eq!(initial_for("日本語", "9xyz"), '9');
    }

    #[test]
    fn all_needed_glyphs_are_nonempty() {
        for ch in ('A'..='Z').chain('0'..='9') {
            let glyph = glyph_for(ch);
            assert!(glyph.iter().any(|row| *row != 0), "glyph {ch} is empty");
        }
    }
}
