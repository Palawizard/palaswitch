//! Tray icon pictures: the number of the active profile, or a crossed-out
//! mouse when none answers, in the colour of the taskbar text.

use crate::model::IconKind;

/// Grey levels from 0 (empty) to 255 (full), row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub width: usize,
    pub height: usize,
    pub levels: Vec<u8>,
}

impl Coverage {
    pub fn empty(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            levels: vec![0; width * height],
        }
    }

    fn at(&self, x: usize, y: usize) -> u8 {
        self.levels[y * self.width + x]
    }

    /// Smallest rectangle holding every covered pixel: (left, top, width,
    /// height), or `None` when nothing is covered.
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let covered = |x: usize, y: usize| self.at(x, y) > 0;
        let rows: Vec<usize> = (0..self.height)
            .filter(|&y| (0..self.width).any(|x| covered(x, y)))
            .collect();
        let columns: Vec<usize> = (0..self.width)
            .filter(|&x| (0..self.height).any(|y| covered(x, y)))
            .collect();
        let (top, bottom) = (*rows.first()?, *rows.last()?);
        let (left, right) = (*columns.first()?, *columns.last()?);
        Some((left, top, right - left + 1, bottom - top + 1))
    }

    /// The covered part of `self`, centred in a `size` by `size` square.
    /// `None` when it does not fit.
    pub fn centred(&self, size: usize) -> Option<Self> {
        let mut out = Self::empty(size, size);
        let Some((left, top, width, height)) = self.bounds() else {
            return Some(out);
        };
        if width > size || height > size {
            return None;
        }
        let (dx, dy) = ((size - width) / 2, (size - height) / 2);
        for y in 0..height {
            for x in 0..width {
                out.levels[(dy + y) * size + dx + x] = self.at(left + x, top + y);
            }
        }
        Some(out)
    }
}

/// Text drawn by the system font, at the largest size that fits `size`.
pub trait TextRenderer {
    fn render(&self, text: &str, size: usize) -> Coverage;
}

/// Colour of the taskbar text: white on a dark taskbar, near black on a
/// light one, as the system icons beside it.
pub fn ink(light_taskbar: bool) -> [u8; 3] {
    if light_taskbar {
        [0x1C, 0x1C, 0x1C]
    } else {
        [0xFF, 0xFF, 0xFF]
    }
}

/// RGBA pixels of the icon for `kind`, `size` pixels square.
pub fn picture(
    kind: IconKind,
    size: usize,
    light_taskbar: bool,
    text: &dyn TextRenderer,
) -> Vec<u8> {
    let coverage = match kind {
        IconKind::Profile(number) => text.render(&number.to_string(), size),
        IconKind::Unknown => text.render("?", size),
        IconKind::NoMouse => crossed_out_mouse(size),
    };
    let [r, g, b] = ink(light_taskbar);
    coverage
        .levels
        .iter()
        .flat_map(|&alpha| [r, g, b, alpha])
        .collect()
}

/// Outline of a mouse with its wheel, crossed out by a diagonal line.
pub fn crossed_out_mouse(size: usize) -> Coverage {
    let s = size as f32;
    let stroke = (s / 9.0).max(1.5);
    let (cx, cy) = (s / 2.0, s / 2.0);
    let (half_width, half_height) = (s * 0.26, s * 0.37);
    let radius = half_width;
    let wheel = ((cx, cy - half_height * 0.66), (cx, cy - half_height * 0.4));
    let slash = ((s * 0.16, s * 0.16), (s * 0.84, s * 0.84));
    let mut coverage = Coverage::empty(size, size);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let body = rounded_rect(px - cx, py - cy, half_width, half_height, radius);
            let outline = body.abs() - stroke / 2.0;
            let wheel = segment(px, py, wheel.0, wheel.1) - stroke / 2.0;
            let slash_line = segment(px, py, slash.0, slash.1);
            // The outline stops short of the slash so that both read apart.
            let gap = stroke * 1.1 - slash_line;
            let mouse = outline.min(wheel).max(gap);
            let distance = mouse.min(slash_line - stroke / 2.0);
            coverage.levels[y * size + x] = level(distance);
        }
    }
    coverage
}

/// Signed distance from a point to a rounded rectangle centred on 0.
fn rounded_rect(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - half_width + radius;
    let qy = y.abs() - half_height + radius;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

/// Distance from a point to the segment from `a` to `b`.
fn segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (px - a.0, py - a.1);
    let t = ((apx * abx + apy * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    (apx - abx * t).hypot(apy - aby * t)
}

/// Pixel coverage for a signed distance: one pixel of anti-aliasing.
fn level(distance: f32) -> u8 {
    ((0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws a filled block as wide as the text is long, for tests that do
    /// not need the system font.
    struct Blocks;

    impl TextRenderer for Blocks {
        fn render(&self, text: &str, size: usize) -> Coverage {
            let mut coverage = Coverage::empty(size, size);
            for level in coverage.levels.iter_mut().take(text.len().min(size)) {
                *level = 255;
            }
            coverage
        }
    }

    #[test]
    fn bounds_and_centring_keep_the_covered_pixels() {
        let mut coverage = Coverage::empty(6, 6);
        coverage.levels[6 + 1] = 10;
        coverage.levels[2 * 6 + 2] = 20;

        let centred = coverage.centred(4).expect("fits");

        assert_eq!(coverage.bounds(), Some((1, 1, 2, 2)));
        assert_eq!(centred.bounds(), Some((1, 1, 2, 2)));
        assert_eq!((centred.at(1, 1), centred.at(2, 2)), (10, 20));
    }

    #[test]
    fn centring_refuses_what_does_not_fit() {
        let mut coverage = Coverage::empty(6, 6);
        coverage.levels[0] = 1;
        coverage.levels[6 * 6 - 1] = 1;

        assert_eq!(coverage.centred(5), None);
    }

    #[test]
    fn crossed_out_mouse_fits_with_its_slash_through_the_middle() {
        for size in [16, 20, 24, 32] {
            let coverage = crossed_out_mouse(size);
            let (left, top, width, height) = coverage.bounds().expect("drawn");

            assert!(left > 0 && top > 0, "touches the edge at {size}");
            assert!(left + width < size && top + height < size, "{size}");
            assert_eq!(coverage.at(size / 2, size / 2), 255, "{size}");
        }
    }

    #[test]
    fn picture_is_drawn_in_the_taskbar_ink() {
        let dark = picture(IconKind::Profile(2), 16, false, &Blocks);
        let light = picture(IconKind::Profile(2), 16, true, &Blocks);

        assert_eq!(dark.len(), 16 * 16 * 4);
        assert_eq!(&dark[..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&light[..4], &[0x1C, 0x1C, 0x1C, 0xFF]);
        assert_eq!(&dark[4..8], &[0xFF, 0xFF, 0xFF, 0x00]);
    }
}
