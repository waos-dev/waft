//! AV1 streams through VideoToolbox, where the machine decodes AV1 in hardware: with no record from
//! their first key frame, and with their file's record.

use objc2_core_media::kCMVideoCodecType_AV1;
use objc2_core_video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
use waft_embedder::video_decoders::{
    CodecKind, DecoderError, EncodedChunk, VideoDecoder, VideoStreamSetup,
};
use waft_video_codecs::av1::test_clip::chunks_of_the_clip;
use waft_video_codecs::av1::{SequenceHeader, obus};

use super::{chunks_of, frames_decoded, options, pixel_format_of, shown_in_order, times_decoded};
use crate::video_decoders::decoder::Decoder;
use crate::video_decoders::session::has_hardware_decoder;

/// The clip's chunks, each picture shown after the one before.
fn chunks() -> Vec<EncodedChunk> {
    let data = chunks_of_the_clip()
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    chunks_of(data, &(0..12).collect::<Vec<i64>>())
}

/// Whether this machine decodes AV1, which VideoToolbox does in hardware alone: Apple
/// silicon from the M3 and the A17 Pro. Elsewhere the tests say so and check nothing.
fn decodes_av1() -> bool {
    let decodes = has_hardware_decoder(kCMVideoCodecType_AV1);
    if !decodes {
        eprintln!("skipped: this machine's VideoToolbox has no AV1 decoder");
    }
    decodes
}

#[test]
fn a_stream_with_no_record_comes_out_in_order_from_its_first_key_frame() {
    if !decodes_av1() {
        return;
    }
    let decoder = Decoder::open(VideoStreamSetup::Av1(None), &options()).expect("a decoder");
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
fn a_stream_with_its_files_record_decodes_in_hardware_from_the_start() {
    if !decodes_av1() {
        return;
    }
    let units = obus(chunks_of_the_clip()[0]).unwrap();
    let sequence = SequenceHeader::among(&units).unwrap().expect("a header");
    let setup = VideoStreamSetup::Av1(Some(sequence.record()));
    let decoder = Decoder::open(setup, &options()).expect("a decoder");
    assert_eq!(decoder.kind(), CodecKind::Hardware);
    assert_eq!(times_decoded(&decoder, chunks()), shown_in_order(12));
}

#[test]
fn a_record_that_is_no_record_is_bad_data() {
    assert!(matches!(
        Decoder::open(VideoStreamSetup::Av1(Some(vec![1, 2])), &options()),
        Err(DecoderError::BadData(_))
    ));
}
