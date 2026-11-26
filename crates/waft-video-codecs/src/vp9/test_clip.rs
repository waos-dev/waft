//! A short VP9 stream, for the decoders' tests: here, and in each host's decoder, which decodes the
//! same clip.

use crate::ivf;

/// Twelve pictures of 320 by 240 in profile 0, the second chunk a superframe that packs an
/// alternate reference, shown never, before its picture, as libvpx made them in two passes
/// (`ffmpeg -f lavfi -i testsrc=size=320x240:rate=30 -frames:v 12 -pix_fmt yuv420p -c:v
/// libvpx-vp9 -g 12 -auto-alt-ref 1 -lag-in-frames 16 -b:v 300k -pass 1 -f null -`, then the
/// same with `-pass 2 -f ivf`), kept in IVF.
pub const CLIP: &[u8] = include_bytes!("test_clip.ivf");

/// The clip's chunks, in the order they are decoded and shown.
pub fn chunks_of_the_clip() -> Vec<&'static [u8]> {
    ivf::frames_of(CLIP)
}
