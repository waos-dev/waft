//! The checks of the codecs beside H.264: HEVC, in Main and Main 10, VP9 and AV1, each a clip of
//! `waft-video-codecs`' that Apple's decoders are checked with too, decoded in the order it is
//! shown. A device whose MediaCodec has no decoder of a codec opens it `Unsupported`; the check
//! then says so and passes, as what the device has rather than what is wrong.

use waft_embedder::video_decoders::{
    CodecChoice, DecoderError, EncodedChunk, NalFormat, VideoDecoders, VideoStreamSetup,
};
use waft_video_codecs::framing::{units_after_start_codes, with_lengths, with_start_codes};
use waft_video_codecs::hevc::{self, ParameterSets, is_parameter_set};
use waft_video_codecs::{av1, vp9};

use crate::checks::{Report, decoded, in_order, in_pictures, options};
use crate::clip::{PICTURE, time_of};

/// When each of the HEVC clips' pictures is shown, in the order they are decoded, counted in
/// pictures: the counts `ffmpeg -bsf:v trace_headers` reads of their slices.
const HEVC_SHOWN_AT: [i64; 12] = [0, 3, 2, 1, 5, 4, 8, 7, 6, 11, 10, 9];
const MAIN_10_SHOWN_AT: [i64; 12] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 11, 10, 9];

/// VP9 and AV1 never reorder: each chunk's picture is shown after the one before.
const SHOWN_AS_DECODED: [i64; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

pub(crate) async fn hevc(decoders: &dyn VideoDecoders) -> Report {
    let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
    let chunks = chunks_of(hevc_pictures(hevc::test_clip::CLIP, |units| {
        with_start_codes(units.iter().copied())
    }));
    in_order_shown(decoders, "HEVC, start codes", setup, chunks, &HEVC_SHOWN_AT).await
}

pub(crate) async fn hevc_after_lengths(decoders: &dyn VideoDecoders) -> Report {
    let sets = ParameterSets::among(&units_after_start_codes(hevc::test_clip::CLIP));
    let setup =
        VideoStreamSetup::Hevc(NalFormat::LengthPrefixed(hevc::test_clip::record_of(&sets)));
    let chunks = chunks_of(hevc_pictures(hevc::test_clip::CLIP, |units| {
        with_lengths(units.iter().copied().filter(|unit| !is_parameter_set(unit)))
    }));
    let title = "HEVC, lengths, with the configuration record";
    in_order_shown(decoders, title, setup, chunks, &HEVC_SHOWN_AT).await
}

/// A stream of ten bits to a sample, which reaches the app in eight, as every frame is RGBA8.
pub(crate) async fn hevc_main_10(decoders: &dyn VideoDecoders) -> Report {
    let setup = VideoStreamSetup::Hevc(NalFormat::StartCodes(None));
    let chunks = chunks_of(hevc_pictures(hevc::test_clip::MAIN_10_CLIP, |units| {
        with_start_codes(units.iter().copied())
    }));
    in_order_shown(decoders, "HEVC Main 10", setup, chunks, &MAIN_10_SHOWN_AT).await
}

pub(crate) async fn vp9(decoders: &dyn VideoDecoders) -> Report {
    let chunks = chunks_of(
        vp9::test_clip::chunks_of_the_clip()
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect(),
    );
    let setup = VideoStreamSetup::Vp9(None);
    in_order_shown(decoders, "VP9", setup, chunks, &SHOWN_AS_DECODED).await
}

pub(crate) async fn av1(decoders: &dyn VideoDecoders) -> Report {
    let chunks = av1::test_clip::chunks_of_the_clip();
    let units = av1::obus(chunks[0]).expect("the clip's first chunk");
    let sequence = av1::SequenceHeader::among(&units)
        .expect("a sequence header that reads")
        .expect("the first chunk's sequence header");
    let setup = VideoStreamSetup::Av1(Some(sequence.record()));
    let chunks = chunks_of(chunks.into_iter().map(<[u8]>::to_vec).collect());
    let title = "AV1, with the configuration record";
    in_order_shown(decoders, title, setup, chunks, &SHOWN_AS_DECODED).await
}

/// `clip`'s pictures in the order they are decoded, each framed by `framed`.
fn hevc_pictures(clip: &[u8], framed: impl Fn(&[&[u8]]) -> Vec<u8>) -> Vec<Vec<u8>> {
    hevc::test_clip::pictures_of(clip)
        .iter()
        .map(|units| framed(units))
        .collect()
}

/// A clip's chunks, each of `data` in the order they are decoded, the first a key frame; their
/// times are given by `in_order_shown`.
fn chunks_of(data: Vec<Vec<u8>>) -> Vec<EncodedChunk> {
    data.into_iter()
        .enumerate()
        .map(|(decoded, data)| EncodedChunk {
            data,
            timestamp: 0,
            duration: Some(PICTURE as u64),
            key_frame: decoded == 0,
        })
        .collect()
}

/// The clip of `chunks` comes out in the order it is shown, the picture of each chunk shown as
/// many pictures in as `shown_at` says, each frame 320 by 240; the first frame's pixels are read
/// back to show.
async fn in_order_shown(
    decoders: &dyn VideoDecoders,
    title: &'static str,
    setup: VideoStreamSetup,
    mut chunks: Vec<EncodedChunk>,
    shown_at: &[i64],
) -> Report {
    let mut report = Report::new(title);
    let decoder = match decoders.open(setup, options(CodecChoice::Any)).await {
        Ok(decoder) => decoder,
        Err(DecoderError::Unsupported(why)) => {
            report.note(format!("skipped: the system has no such decoder ({why})"));
            return report;
        }
        Err(error) => return report.failed(format!("no decoder: {error:?}")),
    };
    report.note(format!("running on {:?}", decoder.kind()));
    for (chunk, shown) in chunks.iter_mut().zip(shown_at) {
        chunk.timestamp = time_of(*shown);
    }
    let frames = match decoded(&*decoder, chunks).await {
        Ok(frames) => frames,
        Err(error) => return report.failed(error),
    };
    let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
    report.expect(
        times == in_order(),
        format!("in the order shown: {:?}", in_pictures(&times)),
    );
    report.expect(
        frames.iter().all(|frame| {
            let rect = frame.visible_rect();
            (rect.width, rect.height) == (320, 240)
        }),
        "every picture is 320 by 240".to_owned(),
    );
    if let Some(first) = frames.first() {
        report.note(format!("colour {:?}", first.color()));
        match first.read_pixels().await {
            Ok(pixels) => report.pictures.push(pixels),
            Err(error) => report.expect(false, format!("the first frame's pixels: {error:?}")),
        }
    }
    report
}
