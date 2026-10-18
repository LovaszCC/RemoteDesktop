//! The remote pointer as the view's cursor. `FreeRDP` hands over the pointer's shape instead of drawing
//! it into the desktop, and the view shows it as its CSS cursor: one pointer, moved by the local mouse
//! without a round trip (`mstsc` and `FreeRDP` do the same).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::session::RdpEvent;

/// A decoded pointer: RGBA, not premultiplied.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pointer<'a> {
    pub width: u16,
    pub height: u16,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub rgba: &'a [u8],
}

/// The [`RdpEvent::PointerShape`] of a pointer, or `None` for an empty or malformed one.
pub(crate) fn shape_event(pointer: &Pointer<'_>) -> Option<RdpEvent> {
    let (width, height) = (pointer.width, pointer.height);
    if width == 0 || height == 0 || pointer.rgba.len() != usize::from(width) * usize::from(height) * 4 {
        return None;
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, u32::from(width), u32::from(height));
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(pointer.rgba).ok()?;
    }
    Some(RdpEvent::PointerShape {
        png: STANDARD.encode(png),
        // A hotspot outside the image makes the whole CSS cursor value invalid.
        hotspot_x: pointer.hotspot_x.min(width - 1),
        hotspot_y: pointer.hotspot_y.min(height - 1),
    })
}

/// The Windows arrow's shape, for the server's "system default pointer": the view's own default would be
/// the local OS's arrow (the Mac one on macOS), which looks out of place over a Windows desktop.
pub(crate) fn default_arrow_event() -> Option<RdpEvent> {
    let rgba = default_arrow();
    shape_event(&Pointer {
        width: 12,
        height: 19,
        hotspot_x: 0,
        hotspot_y: 0,
        rgba: &rgba,
    })
}

/// The Windows arrow, 12×19 RGBA.
fn default_arrow() -> Vec<u8> {
    const ARROW: [&str; 19] = [
        "X...........",
        "XX..........",
        "XOX.........",
        "XOOX........",
        "XOOOX.......",
        "XOOOOX......",
        "XOOOOOX.....",
        "XOOOOOOX....",
        "XOOOOOOOX...",
        "XOOOOOOOOX..",
        "XOOOOOOOOOX.",
        "XOOOOOOXXXXX",
        "XOOOXOOX....",
        "XOOXXOOX....",
        "XOX..XOOX...",
        "XX...XOOX...",
        "X.....XOOX..",
        "......XOOX..",
        ".......XX...",
    ];
    ARROW
        .iter()
        .flat_map(|row| row.bytes())
        .flat_map(|pixel| match pixel {
            b'X' => [0, 0, 0, 255],
            b'O' => [255, 255, 255, 255],
            _ => [0, 0, 0, 0],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(width: u16, height: u16) -> Vec<u8> {
        (0..u32::from(width) * u32::from(height) * 4)
            .map(|i| (i % 251) as u8)
            .collect()
    }

    fn pointer(rgba: &[u8], width: u16, height: u16, hotspot: (u16, u16)) -> Pointer<'_> {
        Pointer {
            width,
            height,
            hotspot_x: hotspot.0,
            hotspot_y: hotspot.1,
            rgba,
        }
    }

    #[test]
    fn encodes_the_bitmap_as_a_png_with_its_hotspot() {
        let data = rgba(3, 2);
        let Some(RdpEvent::PointerShape {
            png,
            hotspot_x,
            hotspot_y,
        }) = shape_event(&pointer(&data, 3, 2, (1, 1)))
        else {
            panic!("no shape");
        };
        assert_eq!((hotspot_x, hotspot_y), (1, 1));
        let bytes = STANDARD.decode(png).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(
            (info.width, info.height, info.color_type),
            (3, 2, png::ColorType::Rgba)
        );
        assert_eq!(&pixels[..info.buffer_size()], &data[..]);
    }

    #[test]
    fn keeps_the_hotspot_inside_the_image() {
        let data = rgba(4, 4);
        let Some(RdpEvent::PointerShape {
            hotspot_x, hotspot_y, ..
        }) = shape_event(&pointer(&data, 4, 4, (9, 4)))
        else {
            panic!("no shape");
        };
        assert_eq!((hotspot_x, hotspot_y), (3, 3));
    }

    #[test]
    fn the_default_arrow_is_a_valid_shape() {
        let arrow = default_arrow();
        assert_eq!(arrow.len(), 12 * 19 * 4);
        assert_eq!(&arrow[..4], &[0, 0, 0, 255], "the tip is black");
        assert!(matches!(
            default_arrow_event(),
            Some(RdpEvent::PointerShape {
                hotspot_x: 0,
                hotspot_y: 0,
                ..
            })
        ));
    }

    #[test]
    fn skips_empty_and_malformed_pointers() {
        assert_eq!(shape_event(&pointer(&[], 0, 0, (0, 0))), None);
        let short = &rgba(2, 2)[1..];
        assert_eq!(shape_event(&pointer(short, 2, 2, (0, 0))), None);
    }
}
