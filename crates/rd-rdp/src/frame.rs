//! Framebuffer frames for the canvas: dirty rectangles collected between ticks, merged, and packed with
//! their RGBA pixels into one binary message.
//!
//! Message layout (little endian):
//!
//! | bytes | field |
//! |---|---|
//! | 4 | `seq`: frame number, from 1 |
//! | 2, 2 | desktop width, height (the canvas size; it changes after a resize) |
//! | 2 | rectangle count |
//! | 2 | reserved, 0 |
//! | per rectangle: 2, 2, 2, 2 | `x`, `y`, `w`, `h` |
//! | per rectangle: `w`·`h`·4 | RGBA rows, top to bottom, alpha 255 |
//!
//! The page paints each rectangle with `putImageData` and acknowledges the frame; at most
//! [`MAX_IN_FLIGHT`] frames are unacknowledged at any time.

/// Frames sent but not yet acknowledged before the session stops sending.
pub const MAX_IN_FLIGHT: u32 = 2;
/// Above this many separate rectangles they collapse into their bounding box.
const MAX_RECTS: usize = 32;
/// Size of the message header.
pub const HEADER_LEN: usize = 12;

/// A rectangle with inclusive edges, as RDP reports updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub left: u16,
    /// Top edge.
    pub top: u16,
    /// Right edge (inclusive).
    pub right: u16,
    /// Bottom edge (inclusive).
    pub bottom: u16,
}

impl Rect {
    /// The whole of a `width`×`height` desktop.
    #[must_use]
    pub fn full(width: u16, height: u16) -> Self {
        Self {
            left: 0,
            top: 0,
            right: width.saturating_sub(1),
            bottom: height.saturating_sub(1),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn touches(self, other: Self) -> bool {
        self.left <= other.right.saturating_add(1)
            && other.left <= self.right.saturating_add(1)
            && self.top <= other.bottom.saturating_add(1)
            && other.top <= self.bottom.saturating_add(1)
    }
}

/// The dirty area since the last frame.
#[derive(Debug, Clone, Default)]
pub struct Dirty {
    rects: Vec<Rect>,
}

impl Dirty {
    /// Adds a rectangle, merging it with every rectangle it overlaps or touches; past 32 rectangles
    /// everything collapses into the bounding box.
    pub fn add(&mut self, mut rect: Rect) {
        while let Some(i) = self.rects.iter().position(|d| d.touches(rect)) {
            rect = rect.union(self.rects.swap_remove(i));
        }
        self.rects.push(rect);
        if self.rects.len() > MAX_RECTS {
            let all = self.rects.iter().copied().reduce(Rect::union);
            self.rects.clear();
            self.rects.extend(all);
        }
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Forgets the dirty area.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// The rectangles.
    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }
}

/// A decoded desktop: RGBA pixels with a row stride.
#[derive(Debug, Clone, Copy)]
pub struct Pixels<'a> {
    /// The pixel data.
    pub data: &'a [u8],
    /// Bytes per row.
    pub stride: usize,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
}

/// Packs the rectangles (clipped to the desktop; those outside are dropped) into a frame message.
#[must_use]
pub fn pack(pixels: Pixels<'_>, rects: &[Rect], seq: u32) -> Vec<u8> {
    let (iw, ih) = (pixels.width, pixels.height);
    let clipped: Vec<(u16, u16, u16, u16)> = rects
        .iter()
        .filter(|r| r.left < iw && r.top < ih && r.left <= r.right && r.top <= r.bottom)
        .map(|r| {
            let right = r.right.min(iw - 1);
            let bottom = r.bottom.min(ih - 1);
            (r.left, r.top, right - r.left + 1, bottom - r.top + 1)
        })
        .collect();
    let body: usize = clipped
        .iter()
        .map(|&(_, _, w, h)| 8 + usize::from(w) * usize::from(h) * 4)
        .sum();
    let mut out = Vec::with_capacity(HEADER_LEN + body);
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&iw.to_le_bytes());
    out.extend_from_slice(&ih.to_le_bytes());
    let count = u16::try_from(clipped.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    for &(x, y, w, h) in clipped.iter().take(usize::from(count)) {
        for v in [x, y, w, h] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let row_bytes = usize::from(w) * 4;
        for row in usize::from(y)..usize::from(y) + usize::from(h) {
            let start = row * pixels.stride + usize::from(x) * 4;
            let at = out.len();
            match pixels.data.get(start..start + row_bytes) {
                Some(src) => out.extend_from_slice(src),
                None => out.resize(at + row_bytes, 0),
            }
            for alpha in out[at..].iter_mut().skip(3).step_by(4) {
                *alpha = 0xFF;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: u16, top: u16, right: u16, bottom: u16) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn overlapping_and_touching_rects_merge() {
        let mut dirty = Dirty::default();
        assert!(dirty.is_empty());
        dirty.add(rect(0, 0, 9, 9));
        dirty.add(rect(10, 0, 19, 9)); // touches
        assert_eq!(dirty.rects(), [rect(0, 0, 19, 9)]);
        dirty.add(rect(100, 100, 109, 109));
        assert_eq!(dirty.rects().len(), 2);
        dirty.add(rect(5, 5, 105, 105)); // bridges both
        assert_eq!(dirty.rects(), [rect(0, 0, 109, 109)]);
        dirty.clear();
        assert!(dirty.is_empty());
    }

    #[test]
    fn many_rects_collapse_into_the_bounding_box() {
        let mut dirty = Dirty::default();
        for i in 0..40u16 {
            dirty.add(rect(i * 10, i * 10, i * 10 + 1, i * 10 + 1));
        }
        assert_eq!(dirty.rects().len(), 8);
        assert_eq!(dirty.rects()[0].left, 0);
        assert!(dirty.rects().iter().any(|r| r.left == 0 && r.right == 321));
    }

    #[test]
    fn pack_writes_the_header_and_clipped_rgba_rows() {
        // A 4×2 desktop, pixel value = its index, alpha 0.
        let (w, h) = (4u16, 2u16);
        let data: Vec<u8> = (0..32u8).map(|i| if i % 4 == 3 { 0 } else { i }).collect();
        let pixels = Pixels {
            data: &data,
            stride: 16,
            width: w,
            height: h,
        };
        let frame = pack(pixels, &[rect(2, 1, 9, 9), rect(9, 9, 10, 10)], 7);
        assert_eq!(&frame[0..4], 7u32.to_le_bytes());
        assert_eq!(&frame[4..8], [4, 0, 2, 0]);
        assert_eq!(&frame[8..12], [1, 0, 0, 0]);
        assert_eq!(&frame[12..20], [2, 0, 1, 0, 2, 0, 1, 0]);
        assert_eq!(&frame[20..], [24, 25, 26, 255, 28, 29, 30, 255]);
        assert_eq!(Rect::full(4, 2), rect(0, 0, 3, 1));
    }
}
