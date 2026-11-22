//! Opening an encoded picture with valo's decoders, as a host that decodes with valo answers
//! `Platform::open_image_codec`: one loader decoding into the host's image store, and its codec
//! adapted to the framework's [`ImageCodec`].

use std::sync::Arc;

use valo_codec::ImageLoader;

use crate::{
    ImageCodec, ImageCodecFuture, ImageDecodeError, ImageFrame, ImageFrameFuture, ImageRepetition,
};

/// `bytes` opened with `loader`, as a host answers `Platform::open_image_codec`: refused when
/// empty, and when the host has no loader, since its renderer has not started.
pub fn open_image_codec(loader: Option<ImageLoader>, bytes: Arc<[u8]>) -> ImageCodecFuture {
    Box::pin(async move {
        if bytes.is_empty() {
            return Err(ImageDecodeError::Empty);
        }
        let loader = loader.ok_or(ImageDecodeError::NoDecoder)?;
        let codec = loader
            .open(
                bytes,
                valo_codec::DecodeOptions {
                    // Pictures are routinely drawn smaller than they decode, and mip levels are
                    // the difference between a smooth downscale and a shimmering one.
                    mipmaps: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(as_decode_error)?;
        Ok(Box::new(ValoImageCodec { codec }) as Box<dyn ImageCodec>)
    })
}

/// Adapts valo's drawable frames to the framework's codec contract.
struct ValoImageCodec {
    codec: valo_codec::Codec,
}

impl ImageCodec for ValoImageCodec {
    fn frame_count(&self) -> u32 {
        self.codec.info().frame_count
    }

    fn repetition(&self) -> ImageRepetition {
        match self.codec.info().repetition {
            valo_codec::Repetition::Once => ImageRepetition::Once,
            valo_codec::Repetition::Times(times) => ImageRepetition::Times(times),
            valo_codec::Repetition::Forever => ImageRepetition::Forever,
        }
    }

    fn next_frame(&mut self) -> ImageFrameFuture<'_> {
        let frame = self.codec.next_frame();
        Box::pin(async move {
            let frame = frame.await.map_err(as_decode_error)?;
            Ok(ImageFrame {
                image: frame.image,
                duration: frame.duration,
            })
        })
    }
}

/// Keeps the decoder's diagnostics at the framework's error boundary.
fn as_decode_error(error: valo_codec::DecodeError) -> ImageDecodeError {
    match error {
        valo_codec::DecodeError::NoDecoder => ImageDecodeError::NoDecoder,
        valo_codec::DecodeError::Unsupported(_) => ImageDecodeError::UnknownFormat,
        valo_codec::DecodeError::InvalidData(reason) => ImageDecodeError::Damaged(reason),
        other => ImageDecodeError::Failed(other.to_string()),
    }
}
