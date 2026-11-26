//! A short AV1 stream, for the decoders' tests: here, and in each host's decoder, which decodes the
//! same clip.

use super::TEMPORAL_DELIMITER;
use crate::ivf;

/// Twelve pictures of 320 by 240 in the Main profile, some decoded in the second chunk and shown
/// later, each in a chunk of its own, as SVT-AV1 made them (`ffmpeg -f lavfi -i
/// testsrc=size=320x240:rate=30 -frames:v 12 -pix_fmt yuv420p -c:v libsvtav1 -g 12 -f ivf`),
/// kept in IVF, which puts a temporal delimiter before each chunk.
pub const CLIP: &[u8] = include_bytes!("test_clip.ivf");

/// The clip's chunks, in the order they are decoded and shown, as MP4 and WebM hold them: each
/// without the temporal delimiter IVF put before it.
pub fn chunks_of_the_clip() -> Vec<&'static [u8]> {
    ivf::frames_of(CLIP)
        .into_iter()
        .map(|frame| {
            let delimiter = [TEMPORAL_DELIMITER << 3 | 0x02, 0];
            frame.strip_prefix(&delimiter[..]).unwrap_or(frame)
        })
        .collect()
}
