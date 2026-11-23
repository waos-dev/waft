//! Pixels encoded as a PNG to look at, by the framework's encoder (`waft_embedder::encode_png`),
//! the same on every machine.

use waft_embedder::valo::PixelBuffer;

/// `pixels`, as `Platform::read_image_pixels` reads them back, encoded as a PNG's bytes, for a
/// test to write to a file.
///
/// # Panics
///
/// Panics if the encoder fails.
pub fn encode_png(pixels: &PixelBuffer) -> Vec<u8> {
    waft_embedder::encode_png(pixels).expect("pixels encode as a PNG")
}
