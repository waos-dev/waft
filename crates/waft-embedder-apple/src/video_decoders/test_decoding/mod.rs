//! What the decoders' tests share: the options a test opens a decoder with, a clip's chunks as an
//! app hands them over, and the times and pixel buffers of the frames a decoder puts out; and the
//! tests of the codecs other than H.264, each a module (H.264's are the decoder's and the
//! session's own).

mod av1;
mod hevc;
mod vp9;

use std::sync::Arc;

use objc2_core_video::CVPixelBufferGetPixelFormatType;
use pollster::block_on;
use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{
    CodecChoice, DecodingPurpose, EncodedChunk, VideoDecoderOptions,
};

use waft_video_codecs::test_decoding::decoded;

use super::decoder::Decoder;
use crate::media::CoreVideoFrame;

/// How long each picture of the clips is shown, in nanoseconds: a thirtieth of a second.
const PICTURE_DURATION: i64 = 33_333_333;

/// The time of a picture shown `number` pictures in, in nanoseconds.
pub(super) fn time_of(number: i64) -> i64 {
    number * PICTURE_DURATION
}

pub(super) fn options() -> VideoDecoderOptions {
    VideoDecoderOptions {
        decoder: CodecChoice::Any,
        purpose: DecodingPurpose::Playback,
        frame_limit: 4,
        largest_size: None,
        default_color: None,
    }
}

/// A clip's chunks, each of `data` in the order they are decoded, the picture each codes shown
/// as many pictures in as `shown_at` says, the first a key frame.
pub(super) fn chunks_of(data: Vec<Vec<u8>>, shown_at: &[i64]) -> Vec<EncodedChunk> {
    assert_eq!(data.len(), shown_at.len(), "a time for each chunk");
    data.into_iter()
        .zip(shown_at)
        .enumerate()
        .map(|(decoded, (data, shown))| EncodedChunk {
            data,
            timestamp: time_of(*shown),
            duration: Some(PICTURE_DURATION as u64),
            key_frame: decoded == 0,
        })
        .collect()
}

/// The times of `count` pictures, shown one after another from the first.
pub(super) fn shown_in_order(count: usize) -> Vec<Option<i64>> {
    (0..count as i64)
        .map(|number| Some(time_of(number)))
        .collect()
}

/// `units` each after a start code, as the clips hold them.
pub(super) fn after_start_codes(units: &[&[u8]]) -> Vec<u8> {
    units
        .iter()
        .flat_map(|unit| [&[0, 0, 0, 1][..], unit].concat())
        .collect()
}

/// Hands every one of `chunks` to `decoder`, drains it, and answers the frames that came out, in
/// the order they came, as [`decoded`] does; a test fails where fewer come out than were handed in.
pub(super) fn frames_decoded(
    decoder: &Decoder,
    chunks: Vec<EncodedChunk>,
) -> Vec<Arc<dyn VideoFrame>> {
    block_on(decoded(decoder, chunks)).expect("a picture for each chunk")
}

/// The times of the frames [`frames_decoded`] answers.
pub(super) fn times_decoded(decoder: &Decoder, chunks: Vec<EncodedChunk>) -> Vec<Option<i64>> {
    frames_decoded(decoder, chunks)
        .iter()
        .map(|frame| frame.timestamp())
        .collect()
}

/// The CoreVideo pixel format of `frame`, a decoder's frame.
pub(super) fn pixel_format_of(frame: &Arc<dyn VideoFrame>) -> u32 {
    let frame: &dyn std::any::Any = &**frame;
    let frame = frame
        .downcast_ref::<CoreVideoFrame>()
        .expect("a VideoToolbox frame");
    CVPixelBufferGetPixelFormatType(frame.pixel_buffer())
}
