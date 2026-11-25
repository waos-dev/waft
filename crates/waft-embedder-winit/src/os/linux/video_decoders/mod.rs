//! The system's video decoders on Linux: GStreamer's, handed H.264 so far.
//!
//! A stream goes through a pipeline of GStreamer's: the chunks pushed into its source, decoded by
//! the system's decoder, VA-API's (`vah264dec`) on the video hardware and libav's (`avdec_h264`)
//! or OpenH264's on the processor, and pulled out of its sink as pictures (`pipeline`). GStreamer's
//! decoders put pictures out in the order they are shown, which is the stream's own; each picture
//! carries the number of the chunk it came from (a reference timestamp), so its frame says what
//! that chunk said, whatever times the app gave (`decoder`). A picture reaches the app as a dmabuf
//! in ARGB8888, Waft's hardware buffer on Linux (`frame`).

mod decoder;
mod frame;
mod output;
mod pipeline;

use std::future::ready;
use std::rc::Rc;
use std::sync::Arc;

use waft_embedder::video_decoders::{
    CodecChoice, DecodeSupport, DecoderError, VideoCodec, VideoDecodeSupport, VideoDecoder,
    VideoDecoderOpenFuture, VideoDecoderOptions, VideoDecoders, VideoStreamDescription,
    VideoStreamSetup,
};

use super::gstreamer::Gst;
use decoder::Decoder;
use frame::FramePool;
use pipeline::Path;

/// The system's video decoders, as `Platform::video_decoders` answers them; `None` where
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
    /// The decoder on the video hardware the registry has, if any.
    fn hardware(&self) -> Option<Path> {
        self.gst
            .has_element("vah264dec")
            .then_some(Path::Hardware("vah264dec"))
    }

    /// The decoder on the processor the registry has, if any: libav's first, which decodes every
    /// profile, then OpenH264's, which decodes the constrained baseline alone.
    fn software(&self) -> Option<Path> {
        self.pool.as_ref()?;
        ["avdec_h264", "openh264dec"]
            .into_iter()
            .find(|name| self.gst.has_element(name))
            .map(Path::Software)
    }

    /// The paths to try for `choice`, in order: the hardware first where any will do.
    fn paths(&self, choice: CodecChoice) -> Vec<Path> {
        match choice {
            CodecChoice::Any => self.hardware().into_iter().chain(self.software()).collect(),
            CodecChoice::Hardware => self.hardware().into_iter().collect(),
            CodecChoice::Software => self.software().into_iter().collect(),
        }
    }
}

impl VideoDecoders for GStreamerDecoders {
    /// GStreamer's registry tells which decoders there are, not which profiles the video
    /// hardware takes, so the hardware is unknown until tried; libav decodes every H.264 profile.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport {
        if description.codec != VideoCodec::H264 {
            return VideoDecodeSupport {
                hardware: DecodeSupport::No,
                software: DecodeSupport::No,
            };
        }
        let hardware = match self.hardware() {
            Some(_) => DecodeSupport::Unknown,
            None => DecodeSupport::No,
        };
        let software = match self.software() {
            Some(Path::Software("avdec_h264")) => DecodeSupport::Yes,
            Some(_) => DecodeSupport::Unknown,
            None => DecodeSupport::No,
        };
        VideoDecodeSupport { hardware, software }
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
    if setup.codec() != VideoCodec::H264 {
        return Err(DecoderError::Unsupported(format!(
            "this system's decoders are handed H.264 alone so far, not {:?}",
            setup.codec()
        )));
    }
    let mut why = DecoderError::Unsupported(format!(
        "GStreamer has no H.264 decoder of the kind chosen ({:?})",
        options.decoder
    ));
    for path in decoders.paths(options.decoder) {
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
        ChromaSampling, CodecKind, DecodingPurpose, NalFormat, PictureSize,
    };

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
        let support = decoders.support(&description(VideoCodec::H264));
        assert_eq!(support.software, DecodeSupport::Yes);
        let hardware = match Gst::get().expect("GStreamer").has_element("vah264dec") {
            true => DecodeSupport::Unknown,
            false => DecodeSupport::No,
        };
        assert_eq!(support.hardware, hardware);
    }

    #[test]
    #[ignore = "needs GStreamer with gst-libav and a render node: cargo test -- --ignored"]
    fn a_codec_gstreamer_is_not_handed_yet_is_not_supported_or_opened() {
        let decoders = video_decoders().expect("GStreamer");
        let support = decoders.support(&description(VideoCodec::Vp9));
        assert_eq!(support.software, DecodeSupport::No);
        assert_eq!(support.hardware, DecodeSupport::No);
        let opened =
            block_on(decoders.open(VideoStreamSetup::Vp9(None), options(CodecChoice::Any)));
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
}
