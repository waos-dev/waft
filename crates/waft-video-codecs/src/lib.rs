//! What a video stream says of itself, read the same way on every system: the part of decoding
//! that belongs to a codec and not to a system. A host's decoder (VideoToolbox, MediaCodec,
//! Media Foundation, GStreamer) opens the system's session and hands it pictures; everything it
//! must know of the stream first is read here, once for all of them, as Chromium's and Firefox's
//! media layers read a stream before their platform decoders see it.
//!
//! Each codec is a module of its own (`h264`, `hevc`, `vp9`, `av1`) that answers the same
//! questions of its stream:
//! - its setup, read from what [`VideoStreamSetup`](waft_embedder::video_decoders::VideoStreamSetup)
//!   carries: how its chunks frame their units ([`framing::Framing`]) and the parameter sets or
//!   configuration it gives ahead;
//! - the units of a chunk (an H.264 or HEVC chunk's units, a VP9 superframe's frames, an AV1
//!   chunk's OBUs), and the parameter sets or sequence header among them;
//! - whether a chunk decodes with nothing before it (`is_key_frame`);
//! - what the stream says of its pictures ([`StreamFacts`]);
//! - where a picture stands in the order its stream is shown in, for a system whose decoder hands
//!   pictures back in the order they were decoded.
//!
//! What is the same for every codec is beside them: framing units after start codes or lengths
//! ([`framing`]), showing pictures in their order ([`presentation_order`]), and a stream read as
//! its chunks come, which a host's decoder asks all of this of ([`stream_reader`]).
//!
//! The invariant: nothing here names a system. A system's needs (VideoToolbox's four-byte
//! lengths, MediaCodec's codec-specific data) are met by the host from what this crate reads.

pub mod av1;
pub mod framing;
pub mod h264;
pub mod hevc;
#[cfg(any(test, feature = "test-support"))]
mod ivf;
pub mod presentation_order;
pub mod stream_reader;
#[cfg(any(test, feature = "test-support"))]
pub mod test_decoding;
pub mod vp9;

use waft_embedder::video_decoders::PictureSize;
use waft_embedder::{PixelAspect, VideoColor};

/// What a stream says of its pictures before any is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamFacts {
    /// How many pictures may be decoded ahead of one shown before them.
    pub reorder_depth: usize,
    /// Bits in each brightness sample: 8, 10 or 12, which a decoder puts out in a layout of 8 bits
    /// or of 16.
    pub bit_depth: u8,
    /// What its samples mean as colour, as far as the stream says.
    pub color: VideoColor,
    /// The shape of one pixel, where the stream says.
    pub pixel_aspect: Option<PixelAspect>,
    /// The pictures' size, where the stream says.
    pub size: Option<PictureSize>,
    /// The profile as the codec numbers it.
    pub profile: u32,
    /// The level as the codec numbers it, where the stream says: VP9 says it only in its file's
    /// configuration record.
    pub level: Option<u32>,
}
