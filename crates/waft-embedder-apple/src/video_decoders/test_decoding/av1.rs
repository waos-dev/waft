//! AV1 streams through VideoToolbox: with no record from their first key frame, and with their
//! file's record. VideoToolbox decodes AV1 in hardware alone (Apple silicon from the M3 and the A17
//! Pro), and its support says only that it may (`Unknown`), as Chrome takes it; a machine without
//! the decoder answers `Unsupported` at the open, or at the first chunk where there is no record
//! to open it with, and a test then says so and checks nothing more.

use std::sync::Arc;

use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
use pollster::block_on;
use waft_embedder::VideoFrame;
use waft_embedder::video_decoders::{
    CodecKind, DecoderError, EncodedChunk, VideoDecoder, VideoStreamSetup,
};
use waft_video_codecs::av1::test_clip::chunks_of_the_clip;
use waft_video_codecs::av1::{SequenceHeader, obus};
use waft_video_codecs::test_decoding::{decoded, now};

use super::{chunks_of, options, pixel_format_of, shown_in_order};
use crate::video_decoders::decoder::Decoder;

/// The clip's chunks, each picture shown after the one before.
fn chunks() -> Vec<EncodedChunk> {
    let data = chunks_of_the_clip()
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    chunks_of(data, &(0..12).collect::<Vec<i64>>())
}

/// A decoder for `setup`; none where VideoToolbox has no AV1 decoder.
fn opened(setup: VideoStreamSetup) -> Option<Decoder> {
    match Decoder::open(setup, &options()) {
        Ok(decoder) => Some(decoder),
        Err(DecoderError::Unsupported(why)) => no_decoder(&why),
        Err(error) => panic!("a decoder: {error:?}"),
    }
}

/// The frames of the clip; none where the decoder ended at the first chunk finding VideoToolbox
/// has no AV1 decoder.
fn frames_of_the_clip(decoder: &Decoder) -> Option<Vec<Arc<dyn VideoFrame>>> {
    match block_on(decoded(decoder, chunks())) {
        Ok(frames) => Some(frames),
        Err(error) => match now(decoder.ended()) {
            Some(DecoderError::Unsupported(why)) => no_decoder(&why),
            _ => panic!("a picture for each chunk: {error}"),
        },
    }
}

fn no_decoder<T>(why: &str) -> Option<T> {
    eprintln!("this machine's VideoToolbox has no AV1 decoder: {why}");
    None
}

#[test]
fn a_stream_with_no_record_comes_out_in_order_from_its_first_key_frame() {
    let Some(decoder) = opened(VideoStreamSetup::Av1(None)) else {
        return;
    };
    let Some(frames) = frames_of_the_clip(&decoder) else {
        return;
    };
    let times: Vec<Option<i64>> = frames.iter().map(|frame| frame.timestamp()).collect();
    assert_eq!(times, shown_in_order(12), "a picture for each chunk");
    for frame in &frames {
        assert_eq!((frame.width(), frame.height()), (320, 240));
        assert_eq!(
            pixel_format_of(frame),
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        );
    }
}

#[test]
fn a_stream_with_its_files_record_decodes_in_hardware_from_the_start() {
    let units = obus(chunks_of_the_clip()[0]).unwrap();
    let sequence = SequenceHeader::among(&units).unwrap().expect("a header");
    let Some(decoder) = opened(VideoStreamSetup::Av1(Some(sequence.record()))) else {
        return;
    };
    assert_eq!(decoder.kind(), CodecKind::Hardware);
    let frames = frames_of_the_clip(&decoder).expect("a decoder opened has its session");
    let times: Vec<Option<i64>> = frames.iter().map(|frame| frame.timestamp()).collect();
    assert_eq!(times, shown_in_order(12));
}

#[test]
fn a_record_that_is_no_record_is_bad_data() {
    assert!(matches!(
        Decoder::open(VideoStreamSetup::Av1(Some(vec![1, 2])), &options()),
        Err(DecoderError::BadData(_))
    ));
}
