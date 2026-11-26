//! The system's video decoders on macOS and iOS: VideoToolbox, handed H.264, HEVC, VP9 and AV1.
//! VP9 is decoded on macOS alone, whose VideoToolbox adds its VP9 decoder for an app that asks,
//! and AV1 in hardware alone, where the machine has it.
//!
//! What the stream says of itself is read by `waft-video-codecs`' reader of its codec; here it
//! reaches VideoToolbox as VideoToolbox takes it: what the stream is configured with as a format
//! (`format`), a chunk as a sample (H.264's and HEVC's units each after four bytes of length, a
//! VP9 or AV1 chunk whole), a picture at a time in one session (`session`).
//! VideoToolbox hands pictures back in the order they were decoded, so they are put in the order
//! they are shown, which is the stream's own, read from each picture, never the times an app
//! gives its chunks; and they reach the app from the queue that decodes (`decoder`, `output`).
//! Every picture comes out 4:2:0 in two planes, as deep as its stream: NV12 for a stream of eight
//! bits to a sample, P010 for a deeper one (HEVC's Main 10, VP9's profile 2, AV1 at ten bits),
//! which its frame's `HardwareBuffer::format` says.

mod decoder;
mod format;
mod output;
mod session;
#[cfg(test)]
mod test_decoding;

use std::future::ready;
use std::rc::Rc;
use std::sync::Arc;

use waft_embedder::video_decoders::{
    DecodeSupport, VideoCodec, VideoDecodeSupport, VideoDecoder, VideoDecoderOpenFuture,
    VideoDecoderOptions, VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};

use objc2_core_media::{
    CMVideoCodecType, kCMVideoCodecType_AV1, kCMVideoCodecType_H264, kCMVideoCodecType_HEVC,
    kCMVideoCodecType_VP9,
};

use decoder::Decoder;
use session::{add_vp9_decoder, has_hardware_decoder};

/// The system's video decoders, as its one GPU's `SystemGpu::video_decoders` answers them.
pub fn video_decoders() -> Option<Rc<dyn VideoDecoders>> {
    Some(Rc::new(VideoToolboxDecoders))
}

struct VideoToolboxDecoders;

impl VideoDecoders for VideoToolboxDecoders {
    /// What VideoToolbox says: whether the system decodes the codec in hardware at all, which
    /// leaves a profile or a size unknown until a decoder is opened; and nothing of software
    /// decoders on macOS, whose VideoToolbox lists none. iOS opens no software decoder, and
    /// VideoToolbox has none of VP8.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport {
        let Some(codec) = codec_type(description.codec) else {
            return VideoDecodeSupport {
                hardware: DecodeSupport::No,
                software: DecodeSupport::No,
            };
        };
        if description.codec == VideoCodec::Vp9 {
            add_vp9_decoder();
        }
        VideoDecodeSupport {
            hardware: if has_hardware_decoder(codec) {
                DecodeSupport::Unknown
            } else {
                DecodeSupport::No
            },
            software: SOFTWARE_SUPPORT,
        }
    }

    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let opened = Decoder::open(setup, &options)
            .map(|decoder| Arc::new(decoder) as Arc<dyn VideoDecoder>);
        Box::pin(ready(opened))
    }
}

/// What is known of the system's software decoders: on macOS, nothing until one is opened; on
/// iOS, that there are none to open.
#[cfg(target_os = "macos")]
const SOFTWARE_SUPPORT: DecodeSupport = DecodeSupport::Unknown;
#[cfg(not(target_os = "macos"))]
const SOFTWARE_SUPPORT: DecodeSupport = DecodeSupport::No;

/// CoreMedia's type of `codec`; none for VP8, of which VideoToolbox has no decoder.
fn codec_type(codec: VideoCodec) -> Option<CMVideoCodecType> {
    match codec {
        VideoCodec::H264 => Some(kCMVideoCodecType_H264),
        VideoCodec::Hevc => Some(kCMVideoCodecType_HEVC),
        VideoCodec::Vp9 => Some(kCMVideoCodecType_VP9),
        VideoCodec::Av1 => Some(kCMVideoCodecType_AV1),
        VideoCodec::Vp8 => None,
    }
}

#[cfg(test)]
mod tests {
    use waft_embedder::video_decoders::{ChromaSampling, PictureSize};

    use super::*;

    fn description(codec: VideoCodec) -> VideoStreamDescription {
        VideoStreamDescription {
            codec,
            profile: None,
            level: None,
            bit_depth: 8,
            chroma: ChromaSampling::Yuv420,
            size: PictureSize {
                width: 1920,
                height: 1080,
            },
            frame_rate: None,
        }
    }

    /// What this machine's VideoToolbox says of each codec, in the test's output: an Apple
    /// silicon Mac from the M3 on decodes all four in hardware.
    #[test]
    fn support_is_what_videotoolbox_says_of_its_hardware() {
        for codec in [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Vp9,
            VideoCodec::Av1,
        ] {
            let support = VideoToolboxDecoders.support(&description(codec));
            eprintln!("{codec:?}: {support:?}");
            let in_hardware = has_hardware_decoder(codec_type(codec).expect("a codec type"));
            assert_eq!(support.hardware == DecodeSupport::Unknown, in_hardware);
            assert_eq!(support.software, SOFTWARE_SUPPORT);
        }
        let vp8 = VideoToolboxDecoders.support(&description(VideoCodec::Vp8));
        assert_eq!(
            (vp8.hardware, vp8.software),
            (DecodeSupport::No, DecodeSupport::No)
        );
    }
}
