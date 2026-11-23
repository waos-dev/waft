//! Pixels read back (`Platform::read_image_pixels`) encoded as a PNG: `Image.toByteData(format:
//! ImageByteFormat.png)`'s encoding, done by valo's software encoder on every host, so a
//! screenshot is the same file wherever the app runs.

use valo::PixelBuffer;

/// `pixels` encoded as a PNG's bytes, or why they could not be.
pub fn encode_png(pixels: &PixelBuffer) -> Result<Vec<u8>, String> {
    valo_codec_software::encode(pixels, valo_codec_software::Encoding::Png)
        .map_err(|error| error.to_string())
}
