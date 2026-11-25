//! The system's video decoders on macOS and iOS: VideoToolbox, handed H.264 so far.
//!
//! What the stream says of itself is read by `waft-video-codecs`; here it reaches VideoToolbox as
//! VideoToolbox takes it: its parameter sets as a format, its units each after four bytes of
//! length, a picture at a time in one session (`session`). VideoToolbox hands pictures back in
//! the order they were decoded, so they are put in the order they are shown, which is the
//! stream's own, read from each picture, never the times an app gives its chunks; and they reach
//! the app from the queue that decodes (`decoder`, `output`).

mod decoder;
mod output;
mod session;

use std::future::ready;
use std::rc::Rc;
use std::sync::Arc;

use waft_embedder::video_decoders::{
    DecodeSupport, VideoCodec, VideoDecodeSupport, VideoDecoder, VideoDecoderOpenFuture,
    VideoDecoderOptions, VideoDecoders, VideoStreamDescription, VideoStreamSetup,
};

use decoder::Decoder;

/// The system's video decoders, as `Platform::video_decoders` answers them.
pub fn video_decoders() -> Option<Rc<dyn VideoDecoders>> {
    Some(Rc::new(VideoToolboxDecoders))
}

struct VideoToolboxDecoders;

impl VideoDecoders for VideoToolboxDecoders {
    /// VideoToolbox says nothing of a profile or a size before a session is made, so H.264 is
    /// unknown until tried, and what is not handed to it yet is not decoded.
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
        let opened = Decoder::open(setup, &options)
            .map(|decoder| Arc::new(decoder) as Arc<dyn VideoDecoder>);
        Box::pin(ready(opened))
    }
}
