//! The window's own pixels, as it shows now: what a report attaches so
//! whoever reads it sees what the operator saw. The app binary reads them
//! from its platform's window (Swarm's `capture`) and hands the hub how
//! ([`Capture`]); an app or test that names none captures nothing.

use ahjo_core::Error;
use gpui::Window;
/// A window's pixels, top row first, four bytes (RGBA) each.
#[derive(Clone)]
pub struct Picture {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Picture {
    /// The picture as a PNG file's bytes.
    pub fn png(&self) -> Result<Vec<u8>, Error> {
        let failed = |e: png::EncodingError| Error::new("capture_failed", e.to_string());
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(&self.rgba).map_err(failed)?;
        writer.finish().map_err(failed)?;
        Ok(out)
    }
}

/// How a window is captured: the app binary's platform capture, a test's
/// own picture under gpui's test platform, which has no pixels, or
/// [`none`].
pub type Capture = fn(&Window) -> Result<Picture, Error>;

/// What an app that names no way to capture answers.
pub fn none(_: &Window) -> Result<Picture, Error> {
    Err(Error::new(
        "capture_unsupported",
        "this app does not capture its window",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_written_as_a_png_of_its_size() {
        let picture = Picture {
            rgba: [10, 20, 30, 255].repeat(6),
            width: 3,
            height: 2,
        };
        let png = picture.png().unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (3, 2));
    }
}
