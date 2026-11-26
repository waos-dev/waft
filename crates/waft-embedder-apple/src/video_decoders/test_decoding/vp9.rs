//! VP9 streams through VideoToolbox, on macOS: with no record from their first key frame, and with
//! their file's record.

use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
use pollster::block_on;
use waft_embedder::video_decoders::{DecoderError, EncodedChunk, VideoDecoder, VideoStreamSetup};
use waft_video_codecs::vp9::test_clip::chunks_of_the_clip;
use waft_video_codecs::vp9::{KeyFrame, Record};

use super::{chunks_of, frames_decoded, options, pixel_format_of, shown_in_order, times_decoded};
use crate::video_decoders::decoder::Decoder;

/// The clip's chunks, each picture shown after the one before.
fn chunks() -> Vec<EncodedChunk> {
    let data = chunks_of_the_clip()
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    chunks_of(data, &(0..12).collect::<Vec<i64>>())
}

#[test]
fn a_stream_with_no_record_comes_out_in_order_from_its_first_key_frame() {
    let decoder = Decoder::open(VideoStreamSetup::Vp9(None), &options()).expect("a decoder");
    let frames = frames_decoded(&decoder, chunks());
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
fn a_stream_with_its_files_record_comes_out_the_same() {
    let key = KeyFrame::of_chunk(chunks_of_the_clip()[0])
        .unwrap()
        .expect("a key frame");
    let record = Record {
        level: 20,
        ..Record::of_key_frame(&key).unwrap()
    };
    let setup = VideoStreamSetup::Vp9(Some(record.to_bytes()));
    let decoder = Decoder::open(setup, &options()).expect("a decoder");
    assert_eq!(times_decoded(&decoder, chunks()), shown_in_order(12));
}

#[test]
fn a_chunk_before_the_first_key_frame_is_bad_data() {
    let decoder = Decoder::open(VideoStreamSetup::Vp9(None), &options()).expect("a decoder");
    let later = chunks().remove(2);
    assert!(matches!(
        block_on(decoder.decode(later)),
        Err(DecoderError::BadData(_))
    ));
}
