//! HEVC streams through VideoToolbox: in start codes and after lengths with their record, and a Main 10
//! stream in P010, each in the order it is shown.

use objc2_core_video::kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange;
use waft_embedder::HardwareBufferFormat;
use waft_embedder::video_decoders::{EncodedChunk, NalFormat, VideoStreamSetup};
use waft_video_codecs::framing::{units_after_start_codes, with_lengths};
use waft_video_codecs::hevc::test_clip::{CLIP, MAIN_10_CLIP, pictures_of, record_of};
use waft_video_codecs::hevc::{ParameterSets, is_parameter_set};

use super::{
    after_start_codes, chunks_of, frames_decoded, options, pixel_format_of, shown_in_order,
    times_decoded,
};
use crate::video_decoders::decoder::Decoder;

/// When each of the clips' pictures is shown, in the order they are decoded, counted in
/// pictures: the counts `ffmpeg -bsf:v trace_headers` reads of their slices.
const SHOWN_AT: [i64; 12] = [0, 3, 2, 1, 5, 4, 8, 7, 6, 11, 10, 9];
const MAIN_10_SHOWN_AT: [i64; 12] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 11, 10, 9];

/// `clip`'s chunks in the order they are decoded, each framed by `framed`.
fn chunks(
    clip: &[u8],
    shown_at: &[i64],
    framed: impl Fn(&[&[u8]]) -> Vec<u8>,
) -> Vec<EncodedChunk> {
    let data = pictures_of(clip)
        .iter()
        .map(|units| framed(units))
        .collect();
    chunks_of(data, shown_at)
}

#[test]
fn a_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
    let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
    let decoder = Decoder::open(setup, &options()).expect("a decoder");
    let times = times_decoded(&decoder, chunks(CLIP, &SHOWN_AT, after_start_codes));
    assert_eq!(times, shown_in_order(12));
}

#[test]
fn a_stream_after_lengths_with_its_record_comes_out_the_same() {
    let sets = ParameterSets::among(&units_after_start_codes(CLIP));
    let setup = VideoStreamSetup::Hevc(NalFormat::LengthPrefixed(record_of(&sets)));
    let decoder = Decoder::open(setup, &options()).expect("a decoder");
    let without_sets = |units: &[&[u8]]| {
        with_lengths(units.iter().copied().filter(|unit| !is_parameter_set(unit)))
    };
    let times = times_decoded(&decoder, chunks(CLIP, &SHOWN_AT, without_sets));
    assert_eq!(times, shown_in_order(12));
}

/// A stream of ten bits to a sample comes out in order as the stream of eight does, its
/// pictures in the layout of 16-bit samples, P010, so none of its bits is lost.
#[test]
fn a_main_10_stream_comes_out_in_order_in_p010() {
    let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
    let decoder = Decoder::open(setup, &options()).expect("a decoder");
    let frames = frames_decoded(
        &decoder,
        chunks(MAIN_10_CLIP, &MAIN_10_SHOWN_AT, after_start_codes),
    );
    let times: Vec<Option<i64>> = frames.iter().map(|frame| frame.timestamp()).collect();
    assert_eq!(times, shown_in_order(12));
    for frame in &frames {
        assert_eq!((frame.width(), frame.height()), (320, 240));
        assert_eq!(
            pixel_format_of(frame),
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
        );
        assert_eq!(frame.hardware_buffer().format(), HardwareBufferFormat::P010);
    }
}
