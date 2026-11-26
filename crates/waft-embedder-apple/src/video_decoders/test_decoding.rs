//! What the decoders' tests share: the options a test opens a decoder with, a clip's chunks as an
//! app hands them over, and the times and pixel buffers of the frames a decoder puts out.

use std::sync::Arc;

use objc2_core_video::CVPixelBufferGetPixelFormatType;
use pollster::block_on;
use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{
    CodecChoice, DecodingPurpose, EncodedChunk, VideoDecoder, VideoDecoderOptions,
};

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
/// the order they came. The chunks are all handed over first, as an app that does not wait for
/// each does, and the frames read as the decoder puts them out.
pub(super) fn frames_decoded(
    decoder: &Decoder,
    chunks: Vec<EncodedChunk>,
) -> Vec<Arc<dyn VideoFrame>> {
    let count = chunks.len();
    let decoding: Vec<_> = chunks
        .into_iter()
        .map(|chunk| decoder.decode(chunk))
        .collect();
    let draining = decoder.drain();
    let frames = (0..count)
        .map(|_| block_on(decoder.next_frame()).expect("a frame"))
        .collect();
    for decoded in decoding {
        block_on(decoded).expect("the chunk decodes");
    }
    block_on(draining).expect("the decoder drains");
    frames
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
