//! The system's video decoders on Android: MediaCodec, through the NDK, handed H.264, HEVC, VP9
//! and AV1.
//!
//! A stream reaches MediaCodec as it takes one, read by `waft-video-codecs`: what the stream is
//! configured with as the codec's format and its codec-specific data, H.264's and HEVC's units
//! each after a start code and a VP9 or AV1 chunk whole, a picture at a time to one codec
//! (`codec`) that draws into an image stream of `PRIVATE` pictures, the system's own hardware
//! buffers. MediaCodec lets its pictures out in the order they are shown, which it reads from the
//! stream itself, so nothing here orders them; each is matched to its chunk by the number the codec
//! carried for it (`pending`), drawn into an RGBA8 frame as a camera's pictures are, and reaches the
//! app from the decoder's own thread (`decoder`, `output`).
//!
//! The NDK has no list of the system's codecs before Android 16 (`MediaCodecList` is Java's, and
//! the NDK's `AMediaCodecStore` came with API 36), so whether a stream decodes is known only once
//! a decoder is opened for it.
//!
//! A stream of ten bits to a sample decodes into the system's own pictures as deep as the codec
//! makes them, and is drawn into an RGBA8 frame as every picture is, so it reaches the app in
//! eight bits.

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
    DecodeSupport, VideoCodec, VideoDecodeSupport, VideoDecodeSupportFuture, VideoDecoder,
    VideoDecoderOpenFuture, VideoDecoderOptions, VideoDecoders, VideoStreamDescription,
    VideoStreamSetup,
};

#[cfg(target_os = "android")]
use decoder::Decoder;

/// The device's video decoders, as its GPU's `SystemGpu::video_decoders` answers them.
#[cfg(target_os = "android")]
pub(crate) struct MediaCodecDecoders;

#[cfg(target_os = "android")]
impl VideoDecoders for MediaCodecDecoders {
    /// The NDK says nothing of the system's codecs before one is made, so every codec handed to
    /// MediaCodec is unknown until tried, and VP8, which is not handed to it, is not decoded.
    fn support(&self, description: &VideoStreamDescription) -> VideoDecodeSupportFuture {
        let support = match description.codec {
            VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Vp9 | VideoCodec::Av1 => {
                DecodeSupport::Unknown
            }
            VideoCodec::Vp8 => DecodeSupport::No,
        };
        Box::pin(std::future::ready(VideoDecodeSupport {
            hardware: support,
            software: support,
        }))
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
