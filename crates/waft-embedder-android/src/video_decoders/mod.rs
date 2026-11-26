//! The system's video decoders on Android: MediaCodec, through the NDK, handed H.264 so far.
//!
//! A stream reaches MediaCodec as it takes one, read by `waft-video-codecs`: its parameter sets
//! as the codec's format, its units each after a start code, a picture at a time to one codec
//! (`codec`) that draws into an image stream of `PRIVATE` pictures, the system's own hardware
//! buffers. MediaCodec lets its pictures out in the order they are shown, which it reads from the
//! stream itself, so nothing here orders them; each is matched to its chunk by the number the codec
//! carried for it (`pending`), drawn into an RGBA8 frame as a camera's pictures are, and reaches the
//! app from the decoder's own thread (`decoder`, `output`).
//!
//! The NDK has no list of the system's codecs (`MediaCodecList` is Java's alone), so whether a
//! stream decodes is known only once a decoder is opened for it.

#[cfg(target_os = "android")]
mod codec;
#[cfg(target_os = "android")]
mod decoder;
#[cfg_attr(
    not(target_os = "android"),
    allow(
        dead_code,
        reason = "read by the decoder, which the host's tests do not build"
    )
)]
mod output;
mod pending;

#[cfg(target_os = "android")]
use std::sync::Arc;

#[cfg(target_os = "android")]
use waft_embedder::video_decoders::{
    DecodeSupport, VideoCodec, VideoDecodeSupport, VideoDecoder, VideoDecoderOpenFuture,
    VideoDecoderOptions, VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};

#[cfg(target_os = "android")]
use decoder::Decoder;

/// The device's video decoders, as its GPU's `SystemGpu::video_decoders` answers them.
#[cfg(target_os = "android")]
pub(crate) struct MediaCodecDecoders;

#[cfg(target_os = "android")]
impl VideoDecoders for MediaCodecDecoders {
    /// The NDK says nothing of a codec's profiles or sizes before one is made, so H.264 is
    /// unknown until tried, and what is not handed to MediaCodec yet is not decoded.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupport {
        let support = match description.codec {
            VideoCodec::H264 => DecodeSupport::Unknown,
            _ => DecodeSupport::No,
        };
        VideoDecodeSupport {
            hardware: support,
            software: support,
        }
    }

    fn open(
        &self,
        setup: VideoStreamSetup,
        options: VideoDecoderOptions,
    ) -> VideoDecoderOpenFuture {
        let opening = Decoder::open(setup, &options);
        Box::pin(async move {
            let decoder = opening.await?;
            Ok(Arc::new(decoder) as Arc<dyn VideoDecoder>)
        })
    }
}
