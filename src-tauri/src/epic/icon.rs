//! Generated initials badges for tray and menu icons.
//!
//! Epic has no local avatar cache (unlike Steam's `avatarcache`), so each
//! account gets a deterministic colored rounded square with its initial:
//! the color is hashed from the account ID, the glyph is rasterized from an
//! embedded UI face (Outfit Regular, SIL OFL 1.1 — see `assets/fonts/OFL.txt`).

use ab_glyph::{point, Font, FontRef, ScaleFont};
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;

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
/// letters are transliterated (Ü -> U, é -> E, …) so the badge alphabet stays
/// A-Z/0-9 — an umlaut's dots vanish into noise at an 18px menu icon anyway.
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

/// Cap height of the glyph as a fraction of the badge size.
const GLYPH_HEIGHT_RATIO: f32 = 0.5;

/// Gamma applied to the rasterizer's coverage before blending. A stroke of
/// this face is barely over a pixel wide at an 18px menu icon, so it is split
/// across two partly covered pixels and linear blending leaves it looking
/// washed out; pulling the midtones up restores the contrast without making
/// the stroke any wider.
const COVERAGE_GAMMA: f32 = 0.75;

/// Draw the initial centered, its cap height scaled to half the badge.
///
/// The letter is rasterized from a real outline with coverage-based
/// anti-aliasing, which is what a badge this small needs: at an 18px menu
/// icon a glyph cell of the old 5×7 bitmap font came out 1.54px wide, so
/// snapping the cells to the pixel grid made every stroke alternate between
/// 1px and 2px — the B grew a double-thick middle bar, the K lost the join of
/// its diagonals. Outlines have no cell grid to snap, so each stroke lands at
/// the same weight and the 20%-of-glyph-width stroke the bitmap font was
/// stuck with drops to what the face actually draws.
fn draw_glyph(img: &mut RgbaImage, ch: char, size: u32) {
    let font = font();
    let cap = size as f32 * GLYPH_HEIGHT_RATIO;
    let px = cap / cap_height_ratio();
    let id = font.glyph_id(ch);
    let Some(ink) = font.outline(id).map(|o| o.bounds) else {
        return;
    };

    // Center the ink horizontally (centering the advance width instead would
    // push a letter with lopsided side bearings off center) and the cap band
    // vertically, so a descender (Q's tail) hangs below without dragging the
    // rest of the alphabet upwards. Both offsets are handed to the rasterizer
    // as fractions of a pixel: rounding them to whole pixels here would dump
    // the half pixel that is left over when the badge and the ink differ by
    // an odd number of pixels entirely on one side — which is what shifted
    // the M half a pixel to the right.
    let scale = font.as_scaled(px).h_scale_factor();
    let x = (size as f32 - ink.width() * scale) / 2.0 - ink.min.x * scale;
    let y = (size as f32 + cap) / 2.0;

    let Some(outlined) = font.outline_glyph(id.with_scale_and_position(px, point(x, y))) else {
        return;
    };
    let bounds = outlined.px_bounds();
    let (left, top) = (bounds.min.x as i32, bounds.min.y as i32);

    outlined.draw(|gx, gy, coverage| {
        let (x, y) = (left + gx as i32, top + gy as i32);
        if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
            return;
        }
        // Blend white into the badge color; the badge is opaque here, so the
        // coverage belongs in the color, not in the alpha the rounded mask
        // is about to apply.
        let alpha = coverage.clamp(0.0, 1.0).powf(COVERAGE_GAMMA);
        let px = img.get_pixel_mut(x as u32, y as u32);
        for channel in &mut px.0[..3] {
            *channel = (*channel as f32 + (255.0 - *channel as f32) * alpha).round() as u8;
        }
    });
}

/// The embedded face, parsed once.
fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| {
        FontRef::try_from_slice(include_bytes!("../../assets/fonts/Outfit-Regular.ttf"))
            .expect("embedded badge font is a valid TTF")
    })
}

/// Cap height of the embedded face, in pixels per unit of `PxScale`, measured
/// once from the unscaled ink of an 'H'. `ab_glyph` exposes no cap-height
/// metric, and the ink is what actually has to fill the badge anyway.
fn cap_height_ratio() -> f32 {
    static RATIO: OnceLock<f32> = OnceLock::new();
    *RATIO.get_or_init(|| {
        let font = font();
        let ink = font
            .outline(font.glyph_id('H'))
            .expect("embedded badge font draws an 'H'")
            .bounds;
        // `Outline::bounds` holds font units, which grow upwards, but names
        // its corners for the y-down box `px_bounds` flips them into — so the
        // cap height is `min.y - max.y` and `height()` would come out negative.
        // A `PxScale` is the face's full line height rather than its em, so the
        // conversion has to go through `ab_glyph`'s own factor: dividing by
        // `units_per_em` overshoots by the descender and line gap.
        (ink.min.y - ink.max.y) * font.as_scaled(1.0).v_scale_factor()
    })
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
        // The glyph paints near-white ink over the badge color.
        let brightest = rgba
            .chunks_exact(4)
            .map(|px| px[0] as u32 + px[1] as u32 + px[2] as u32)
            .max()
            .expect("badge is not empty");
        assert!(brightest >= 3 * 240, "expected near-white glyph ink");
    }

    /// Per-pixel ink coverage, 0.0 where the badge color shows through and
    /// 1.0 where the glyph painted solid white. The corner pixel is never
    /// touched by the glyph and the rounded mask only edits alpha, so its RGB
    /// is the untouched background color.
    fn coverage(rgba: &[u8], size: u32) -> Vec<f32> {
        let bg: Vec<f32> = rgba[0..3].iter().map(|c| *c as f32).collect();
        (0..(size * size) as usize)
            .map(|i| {
                let px = &rgba[i * 4..i * 4 + 3];
                let lit: f32 = (0..3)
                    .map(|c| (px[c] as f32 - bg[c]) / (255.0 - bg[c]).max(1.0))
                    .sum();
                (lit / 3.0).clamp(0.0, 1.0)
            })
            .collect()
    }

    /// Half-open bounding box `(x0, y0, x1, y1)` of the pixels the glyph
    /// painted over the flat background.
    fn lit_bounds(rgba: &[u8], size: u32) -> (u32, u32, u32, u32) {
        let cov = coverage(rgba, size);
        let (mut x0, mut y0, mut x1, mut y1) = (size, size, 0, 0);
        for y in 0..size {
            for x in 0..size {
                if cov[(y * size + x) as usize] > 0.0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        (x0, y0, x1, y1)
    }

    /// Coverage-weighted center of the ink, in pixels from the badge center.
    fn ink_offset(rgba: &[u8], size: u32) -> (f32, f32) {
        let cov = coverage(rgba, size);
        let (mut total, mut wx, mut wy) = (0.0, 0.0, 0.0);
        for y in 0..size {
            for x in 0..size {
                let c = cov[(y * size + x) as usize];
                total += c;
                wx += c * (x as f32 + 0.5);
                wy += c * (y as f32 + 0.5);
            }
        }
        let center = size as f32 / 2.0;
        (wx / total - center, wy / total - center)
    }

    #[test]
    fn glyph_is_centered_at_every_size() {
        // Measured on the ink's center of mass, not its bounding box: placing
        // the glyph on whole pixels used to leave the M half a pixel right of
        // center, which a box that rounds outwards to the same margins on
        // both sides cannot see. Sizes are picked so the outline lands both on
        // and off pixel boundaries; the letters are the ones symmetric about
        // their vertical axis, so their mass belongs dead center horizontally.
        for size in [16u32, 18, 20, 24, 32, 64] {
            for ch in ['M', 'H', 'O', 'A', 'W', 'X', 'T', 'I'] {
                let (rgba, _) = badge_rgba("center", ch, size);
                let (dx, _) = ink_offset(&rgba, size);
                assert!(
                    dx.abs() <= 0.1,
                    "size {size}: '{ch}' sits {dx:+.3}px off horizontal center"
                );
            }
            // Vertically the cap band is what is centered, and only letters
            // that fill it evenly can be checked by their mass — an 'A' is a
            // triangle and a 'T' a bar on a stem, so both weigh in off center
            // by design.
            for ch in ['H', 'O', 'X'] {
                let (rgba, _) = badge_rgba("center", ch, size);
                let (_, dy) = ink_offset(&rgba, size);
                assert!(
                    dy.abs() <= 0.2,
                    "size {size}: '{ch}' sits {dy:+.3}px off vertical center"
                );
            }
        }
    }

    #[test]
    fn glyph_fills_the_intended_share_of_the_badge() {
        for size in [16u32, 18, 20, 24, 32, 64] {
            let (rgba, _) = badge_rgba("height", 'H', size);
            let (_, y0, _, y1) = lit_bounds(&rgba, size);
            // 'H' has no overshoot, so its ink height is the cap height, up
            // to the anti-aliased fringe and the rounded placement.
            let want = size as f32 * GLYPH_HEIGHT_RATIO;
            let got = (y1 - y0) as f32;
            assert!(
                (got - want).abs() <= 1.5,
                "size {size}: glyph height {got} is not ~{want}"
            );
        }
    }

    #[test]
    fn strokes_keep_an_even_weight() {
        // The bug this replaced: a bitmap font scaled by a fractional factor
        // snapped its cells to the grid, so the two stems of an 'H' came out
        // 2px and 1px wide on the same badge. Both stems are measured on the
        // row through the crossbar-free upper half.
        for size in [16u32, 18, 20, 24, 32] {
            let (rgba, _) = badge_rgba("weight", 'H', size);
            let (_, y0, _, y1) = lit_bounds(&rgba, size);
            let row = y0 + (y1 - y0) / 5;
            let cov = coverage(&rgba, size);
            let stems: Vec<f32> = cov[(row * size) as usize..((row + 1) * size) as usize]
                .split(|c| *c <= 0.0)
                .filter(|run| !run.is_empty())
                .map(|run| run.iter().sum())
                .collect();
            assert_eq!(stems.len(), 2, "size {size}: expected two stems of 'H'");
            assert!(
                (stems[0] - stems[1]).abs() <= 0.35,
                "size {size}: stems weigh {:?} — uneven",
                stems
            );
        }
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
            let (rgba, size) = badge_rgba("glyphs", ch, 32);
            let (x0, y0, x1, y1) = lit_bounds(&rgba, size);
            assert!(x1 > x0 && y1 > y0, "glyph {ch} painted nothing");
        }
    }
}
