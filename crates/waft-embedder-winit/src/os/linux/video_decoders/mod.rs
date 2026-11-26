//! The system's video decoders on Linux: GStreamer's, handed H.264, HEVC, VP9 and AV1.
//!
//! A stream goes through a pipeline of GStreamer's: the chunks pushed into its source, decoded by
//! the system's decoder of its codec, VA-API's on the video hardware (`vah264dec`, `vah265dec`,
//! `vavp9dec` and `vaav1dec`, the last two after their parsers) and libav's, OpenH264's or dav1d's
//! on the processor, and pulled out of
//! its sink as pictures (`pipeline`). GStreamer's decoders read the stream themselves, as its caps
//! say it is framed, so nothing of `waft-video-codecs` is asked here. GStreamer's
//! decoders put pictures out in the order they are shown, which is the stream's own; each picture
//! carries the number of the chunk it came from (a reference timestamp), so its frame says what
//! that chunk said, whatever times the app gave (`decoder`). A picture reaches the app as a dmabuf
//! in ARGB8888, Waft's hardware buffer on Linux (`frame`), so a stream of ten bits to a sample
//! reaches it in eight.

mod decoder;
mod frame;
mod output;
mod pipeline;

use std::future::ready;
use std::rc::Rc;
use std::sync::Arc;

use waft_embedder::video_decoders::{
    CodecChoice, DecodeSupport, DecoderError, VideoCodec, VideoDecodeSupport,
    VideoDecodeSupportFuture, VideoDecoder, VideoDecoderOpenFuture, VideoDecoderOptions,
    VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};

/// VA-API's decoder of each codec GStreamer is handed, on the video hardware, after the parser it
/// needs: VP9's decoder takes no superframe, and AV1's a frame at a time, not a temporal unit.
fn hardware_decoder(codec: VideoCodec) -> Option<&'static [&'static str]> {
    match codec {
        VideoCodec::H264 => Some(&["vah264dec"]),
        VideoCodec::Hevc => Some(&["vah265dec"]),
        VideoCodec::Vp9 => Some(&["vp9parse", "vavp9dec"]),
        VideoCodec::Av1 => Some(&["av1parse", "vaav1dec"]),
        VideoCodec::Vp8 => None,
    }
}

/// The decoders of each codec on the processor, the first the registry has taken: libav's for
/// H.264, HEVC and VP9, then OpenH264's, which decodes H.264's constrained baseline alone; dav1d's
/// for AV1, of which gst-libav makes no decoder.
fn software_decoders(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["avdec_h264", "openh264dec"],
        VideoCodec::Hevc => &["avdec_h265"],
        VideoCodec::Vp9 => &["avdec_vp9"],
        VideoCodec::Av1 => &["dav1ddec"],
        VideoCodec::Vp8 => &[],
    }
}

/// The decoders on the processor that decode every profile of their codec: libav's of H.264
/// and VP9, and dav1d.
const EVERY_PROFILE: [&str; 3] = ["avdec_h264", "avdec_vp9", "dav1ddec"];

use super::gstreamer::Gst;
use decoder::Decoder;
use frame::FramePool;
use pipeline::Path;

/// The system's video decoders, as the GPU's `SystemGpu::video_decoders` answers them; `None` where
/// GStreamer is missing.
pub(crate) fn video_decoders() -> Option<Rc<dyn VideoDecoders>> {
    let gst = Gst::get()?;
    Some(Rc::new(GStreamerDecoders {
        gst,
        pool: FramePool::new().map(Arc::new),
    }))
}

struct GStreamerDecoders {
    gst: &'static Gst,
    /// Where pictures decoded on the processor are copied to; none where no render node opens,
    /// and then only the video hardware decodes.
    pool: Option<Arc<FramePool>>,
}

impl GStreamerDecoders {
    /// The decoder of `codec` on the video hardware the registry has, if any.
    fn hardware(&self, codec: VideoCodec) -> Option<Path> {
        hardware_decoder(codec)
            .filter(|elements| elements.iter().all(|name| self.gst.has_element(name)))
            .map(Path::Hardware)
    }

    /// The decoder of `codec` on the processor the registry has, if any.
    fn software(&self, codec: VideoCodec) -> Option<Path> {
        self.pool.as_ref()?;
        software_decoders(codec)
            .iter()
            .copied()
            .find(|name| self.gst.has_element(name))
            .map(Path::Software)
    }

    /// The paths to try for `codec` and `choice`, in order: the hardware first where any will do.
    fn paths(&self, codec: VideoCodec, choice: CodecChoice) -> Vec<Path> {
        match choice {
            CodecChoice::Any => self
                .hardware(codec)
                .into_iter()
                .chain(self.software(codec))
                .collect(),
            CodecChoice::Hardware => self.hardware(codec).into_iter().collect(),
            CodecChoice::Software => self.software(codec).into_iter().collect(),
        }
    }
}

impl VideoDecoders for GStreamerDecoders {
    /// GStreamer's registry tells which decoders there are, not which profiles the video
    /// hardware takes, so the hardware is unknown until tried; libav decodes every H.264 and VP9
    /// profile and dav1d every AV1 profile, and the others are unknown until tried.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupportFuture {
        let codec = description.codec;
        let hardware = match self.hardware(codec) {
            Some(_) => DecodeSupport::Unknown,
            None => DecodeSupport::No,
        };
        let software = match self.software(codec) {
            Some(Path::Software(name)) if EVERY_PROFILE.contains(&name) => DecodeSupport::Yes,
            Some(_) => DecodeSupport::Unknown,
            None => DecodeSupport::No,
        };
        Box::pin(ready(VideoDecodeSupport { hardware, software }))
    }

    /// A decoder on the first path that starts: the video hardware's, where one is there and
    /// the choice allows it, and the processor's otherwise.
    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let opened = open_first(self, setup, &options);
        Box::pin(ready(opened))
    }
}

fn open_first(
    decoders: &GStreamerDecoders,
    setup: VideoStreamSetup,
    options: &VideoDecoderOptions,
) -> Result<Arc<dyn VideoDecoder>, DecoderError> {
    let codec = setup.codec();
    let mut why = DecoderError::Unsupported(format!(
        "GStreamer has no {codec:?} decoder of the kind chosen ({:?})",
        options.decoder
    ));
    for path in decoders.paths(codec, options.decoder) {
        match Decoder::open(
            decoders.gst,
            path,
            decoders.pool.clone(),
            setup.clone(),
            options,
        ) {
            Ok(decoder) => return Ok(Arc::new(decoder)),
            Err(error) => why = error,
        }
    }
    Err(why)
}

#[cfg(test)]
mod tests {
    use pollster::block_on;
    use waft_embedder::video_decoders::{
        ChromaSampling, CodecKind, DecodingPurpose, EncodedChunk, NalFormat, PictureSize,
    };
    use waft_video_codecs::framing::{units_after_start_codes, with_lengths, with_start_codes};
    use waft_video_codecs::{av1, hevc, vp9};

    use super::*;

    fn description(codec: VideoCodec) -> VideoStreamDescription {
        VideoStreamDescription {
            codec,
            profile: Some(100),
            level: Some(40),
            bit_depth: 8,
            chroma: ChromaSampling::Yuv420,
            size: PictureSize {
                width: 1920,
                height: 1080,
            },
            frame_rate: Some(30.0),
        }
    }

    fn options(decoder: CodecChoice) -> VideoDecoderOptions {
        VideoDecoderOptions {
            decoder,
            purpose: DecodingPurpose::Playback,
            frame_limit: 4,
            largest_size: None,
            default_color: None,
        }
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn libav_decodes_h264_on_the_processor_and_the_registry_tells_the_hardware() {
        let decoders = video_decoders().expect("GStreamer");
        let support = block_on(decoders.support(&description(VideoCodec::H264)));
        assert_eq!(support.software, DecodeSupport::Yes);
        let hardware = match Gst::get().expect("GStreamer").has_element("vah264dec") {
            true => DecodeSupport::Unknown,
            false => DecodeSupport::No,
        };
        assert_eq!(support.hardware, hardware);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_codec_gstreamer_is_not_handed_is_not_supported_or_opened() {
        let decoders = video_decoders().expect("GStreamer");
        let support = block_on(decoders.support(&description(VideoCodec::Vp8)));
        assert_eq!(support.software, DecodeSupport::No);
        assert_eq!(support.hardware, DecodeSupport::No);
        let opened = block_on(decoders.open(VideoStreamSetup::Vp8, options(CodecChoice::Any)));
        assert!(matches!(opened, Err(DecoderError::Unsupported(_))));
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_decoder_runs_where_its_choice_asks() {
        let decoders = video_decoders().expect("GStreamer");
        let setup = VideoStreamSetup::H264(NalFormat::StartCodes(None));
        let software = block_on(decoders.open(setup.clone(), options(CodecChoice::Software)))
            .expect("a decoder on the processor");
        assert_eq!(software.kind(), CodecKind::Software);
        let hardware = block_on(decoders.open(setup, options(CodecChoice::Hardware)));
        match hardware {
            Ok(decoder) => assert_eq!(decoder.kind(), CodecKind::Hardware),
            Err(error) => assert!(
                matches!(error, DecoderError::Unsupported(_)),
                "no video hardware is unsupported, not {error:?}"
            ),
        }
    }

    /// The times of a picture shown `number` pictures in, in nanoseconds, at thirty a second.
    fn time_of(number: i64) -> i64 {
        number * 33_333_333
    }

    /// A clip's chunks, each of `data` in the order they are decoded, the picture of each shown as
    /// many pictures in as `shown_at` says, the first a key frame.
    fn chunks_of(data: Vec<Vec<u8>>, shown_at: &[i64]) -> Vec<EncodedChunk> {
        data.into_iter()
            .zip(shown_at)
            .enumerate()
            .map(|(decoded, (data, shown))| EncodedChunk {
                data,
                timestamp: time_of(*shown),
                duration: Some(33_333_333),
                key_frame: decoded == 0,
            })
            .collect()
    }

    /// Decodes `chunks` of the stream `setup` begins on each kind of decoder the registry has,
    /// and checks that each comes out in the order it is shown, of 320 by 240; says so where the
    /// registry has none of a kind.
    fn decodes_in_order_on_each_kind(setup: VideoStreamSetup, chunks: Vec<EncodedChunk>) {
        let decoders = video_decoders().expect("GStreamer");
        for choice in [CodecChoice::Hardware, CodecChoice::Software] {
            let decoder = match block_on(decoders.open(setup.clone(), options(choice))) {
                Ok(decoder) => decoder,
                Err(DecoderError::Unsupported(why)) => {
                    eprintln!("skipped {choice:?} for {:?}: {why}", setup.codec());
                    continue;
                }
                Err(error) => panic!("no {choice:?} decoder: {error:?}"),
            };
            let count = chunks.len();
            let decoding: Vec<_> = chunks
                .iter()
                .cloned()
                .map(|chunk| decoder.decode(chunk))
                .collect();
            let draining = decoder.drain();
            let frames: Vec<_> = (0..count)
                .map(|_| block_on(decoder.next_frame()).expect("a frame"))
                .collect();
            for decoded in decoding {
                block_on(decoded).expect("the chunk decodes");
            }
            block_on(draining).expect("the decoder drains");
            let times: Vec<_> = frames.iter().map(|frame| frame.timestamp()).collect();
            let in_order: Vec<_> = (0..count as i64)
                .map(|number| Some(time_of(number)))
                .collect();
            assert_eq!(times, in_order, "{choice:?}");
            for frame in &frames {
                assert_eq!((frame.width(), frame.height()), (320, 240));
            }
        }
    }

    /// When each of the HEVC clips' pictures is shown, in the order they are decoded, counted in
    /// pictures: the counts `ffmpeg -bsf:v trace_headers` reads of their slices.
    const HEVC_SHOWN_AT: [i64; 12] = [0, 3, 2, 1, 5, 4, 8, 7, 6, 11, 10, 9];
    const MAIN_10_SHOWN_AT: [i64; 12] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 11, 10, 9];

    /// VP9 and AV1 never reorder: each chunk's picture is shown after the one before.
    const SHOWN_AS_DECODED: [i64; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

    fn hevc_chunks(
        clip: &[u8],
        shown_at: &[i64],
        framed: impl Fn(&[&[u8]]) -> Vec<u8>,
    ) -> Vec<EncodedChunk> {
        let data = hevc::test_clip::pictures_of(clip)
            .iter()
            .map(|units| framed(units))
            .collect();
        chunks_of(data, shown_at)
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn an_hevc_stream_in_start_codes_comes_out_in_the_order_it_is_shown() {
        let chunks = hevc_chunks(hevc::test_clip::CLIP, &HEVC_SHOWN_AT, |units| {
            with_start_codes(units.iter().copied())
        });
        decodes_in_order_on_each_kind(VideoStreamSetup::Hevc(NalFormat::StartCodes(None)), chunks);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn an_hevc_stream_after_lengths_with_its_record_comes_out_the_same() {
        let clip = hevc::test_clip::CLIP;
        let sets = hevc::ParameterSets::among(&units_after_start_codes(clip));
        let setup =
            VideoStreamSetup::Hevc(NalFormat::LengthPrefixed(hevc::test_clip::record_of(&sets)));
        let chunks = hevc_chunks(clip, &HEVC_SHOWN_AT, |units| {
            with_lengths(
                units
                    .iter()
                    .copied()
                    .filter(|unit| !hevc::is_parameter_set(unit)),
            )
        });
        decodes_in_order_on_each_kind(setup, chunks);
    }

    /// A stream of ten bits to a sample comes out in order as the stream of eight does, each
    /// picture converted to ARGB8888.
    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn an_hevc_main_10_stream_comes_out_in_the_order_it_is_shown() {
        let chunks = hevc_chunks(hevc::test_clip::MAIN_10_CLIP, &MAIN_10_SHOWN_AT, |units| {
            with_start_codes(units.iter().copied())
        });
        decodes_in_order_on_each_kind(VideoStreamSetup::Hevc(NalFormat::StartCodes(None)), chunks);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_vp9_stream_comes_out_in_order_from_its_first_key_frame() {
        let data = vp9::test_clip::chunks_of_the_clip()
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect();
        decodes_in_order_on_each_kind(
            VideoStreamSetup::Vp9(None),
            chunks_of(data, &SHOWN_AS_DECODED),
        );
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn an_av1_stream_comes_out_in_order_with_and_without_its_record() {
        let clip = av1::test_clip::chunks_of_the_clip();
        let units = av1::obus(clip[0]).expect("the first chunk's units");
        let sequence = av1::SequenceHeader::among(&units)
            .expect("a header that reads")
            .expect("the first chunk's sequence header");
        let data: Vec<Vec<u8>> = clip.into_iter().map(<[u8]>::to_vec).collect();
        for setup in [
            VideoStreamSetup::Av1(None),
            VideoStreamSetup::Av1(Some(sequence.record())),
        ] {
            decodes_in_order_on_each_kind(setup, chunks_of(data.clone(), &SHOWN_AS_DECODED));
        }
    }
}
