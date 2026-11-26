//! One decoder of the system's, as the NDK makes it: an `AMediaCodec` of the stream's codec,
//! configured with what the stream's description says, drawing its pictures into the window of an
//! image stream of `PRIVATE` pictures, as Android's own players draw into a surface. A picture
//! comes out of the codec as an output buffer, in the order pictures are shown, which the codec
//! reads from the stream; it is let into the stream by rendering it, and reaches what the codec was
//! made with on the stream's own thread, as an image whose memory is its `AHardwareBuffer`.
//!
//! What the codec is configured with is its codec-specific data, as MediaCodec documents it:
//! H.264's sequence sets as `csd-0` and its picture sets as `csd-1`, HEVC's video, sequence and
//! picture sets together as `csd-0`, each set after a start code; AV1's configuration record
//! (`av1C`) as `csd-0`; and nothing for VP9, whose key frames say all its decoder needs.
//!
//! The codec is handed each chunk with a number for its presentation time, which the picture's
//! image carries back (in nanoseconds, a thousand times the microseconds it was handed): what tells
//! which chunk a picture is of, whatever times the app gives its chunks.
//!
//! The invariant: the codec is made, fed and dropped on one thread, the decoder's own, and dropped
//! before the stream whose window it draws into. A picture rendered before the stream took the last
//! takes its place, so its caller renders the next only once the last was delivered.

use std::time::Duration;

use ndk::hardware_buffer::HardwareBufferUsage;
use ndk::media::image_reader::ImageFormat;
use ndk::media::media_codec::{
    DequeuedInputBufferResult, DequeuedOutputBufferInfoResult, MediaCodec, MediaCodecDirection,
};
use ndk::media::media_format::MediaFormat;
use ndk::media_error::MediaError;
use waft_embedder::video_decoders::{
    CodecChoice, CodecKind, DecoderError, DecodingPurpose, PictureSize, VideoCodec,
};
use waft_video_codecs::framing::with_start_codes;
use waft_video_codecs::stream_reader::Configuration;

use crate::media::image_stream::{Delivery, ImageStream, StreamImage};

/// The MIME type MediaCodec names `codec` by; none for VP8, which is not handed to it.
fn mime_type(codec: VideoCodec) -> Option<&'static str> {
    match codec {
        VideoCodec::H264 => Some("video/avc"),
        VideoCodec::Hevc => Some("video/hevc"),
        VideoCodec::Vp9 => Some("video/x-vnd.on2.vp9"),
        VideoCodec::Av1 => Some("video/av01"),
        VideoCodec::Vp8 => None,
    }
}

/// The system's software decoders of `codec`, by the names Codec2 and the older OMX give them:
/// OMX has none of AV1.
fn software_decoders(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["c2.android.avc.decoder", "OMX.google.h264.decoder"],
        VideoCodec::Hevc => &["c2.android.hevc.decoder", "OMX.google.hevc.decoder"],
        VideoCodec::Vp9 => &["c2.android.vp9.decoder", "OMX.google.vp9.decoder"],
        VideoCodec::Av1 => &["c2.android.av1.decoder"],
        VideoCodec::Vp8 => &[],
    }
}

/// The prefixes of a software decoder's name: the platform's own codecs.
const SOFTWARE_PREFIXES: [&str; 2] = ["c2.android.", "OMX.google."];

/// How many of its pictures the stream lends at once: each is drawn into a frame of its own and
/// given back at once, so a few keep the codec from waiting for a slot.
const PICTURES_LENT: i32 = 4;

/// `AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG`.
const CODEC_CONFIG: u32 = 2;

/// `AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM`.
const END_OF_STREAM: u32 = 4;

/// What a decoder is made for, beside its stream.
#[derive(Clone, Copy)]
pub(super) struct CodecOptions {
    pub(super) choice: CodecChoice,
    pub(super) purpose: DecodingPurpose,
    pub(super) largest_size: Option<PictureSize>,
}

/// What one input buffer carries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Input {
    /// A chunk, by its number.
    Chunk(u64),
    /// What the stream goes on with, as it configures the codec: H.264's and HEVC's parameter
    /// sets.
    Configuration,
    /// Nothing more: the codec lets out every picture it holds.
    EndOfStream,
}

/// What came out of the codec at one ask.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Coming {
    /// The picture of the chunk of this number, rendered into the stream or dropped.
    Picture { number: u64, rendered: bool },
    /// Every chunk handed over before the end of the stream has come out.
    EndOfStream,
    /// Nothing yet.
    Nothing,
}

/// A codec and the stream its pictures are drawn into.
pub(super) struct Codec {
    // Declared before the stream, so it is dropped first: it draws into the stream's window.
    codec: MediaCodec,
    kind: CodecKind,
    /// The largest pictures it was configured to take.
    largest: PictureSize,
    _pictures: ImageStream,
}

impl Codec {
    /// A decoder of the stream `configuration` begins, whose pictures are of `size`, of the kind
    /// `options` chooses, each picture handed to `delivered` on the stream's thread, in the order
    /// the codec renders them.
    pub(super) fn open(
        configuration: &Configuration<'_>,
        size: PictureSize,
        options: CodecOptions,
        delivered: impl Fn(StreamImage) + Send + 'static,
    ) -> Result<Codec, DecoderError> {
        let codec_type = codec_of(configuration);
        let mime = mime_type(codec_type).ok_or_else(|| {
            DecoderError::Unsupported(format!("MediaCodec is not handed {codec_type:?}"))
        })?;
        let codec = made(codec_type, mime, options.choice)?;
        let kind = kind_of(&codec);
        if options.choice == CodecChoice::Hardware && kind == CodecKind::Software {
            return Err(DecoderError::Unsupported(format!(
                "the system has no {codec_type:?} decoder on its video hardware"
            )));
        }
        let largest = largest(size, options.largest_size);
        let pictures = ImageStream::new(
            [size.width, size.height],
            ImageFormat::PRIVATE,
            HardwareBufferUsage::GPU_SAMPLED_IMAGE,
            PICTURES_LENT,
            Delivery::Each,
            delivered,
        )
        .map_err(|error| {
            DecoderError::Failed(format!("no stream to draw pictures into: {error}"))
        })?;
        let window = pictures
            .window()
            .map_err(|error| DecoderError::Failed(format!("no window to draw into: {error}")))?;
        let format = format_of(mime, configuration, size, largest, options.purpose);
        codec
            .configure(&format, Some(&window), MediaCodecDirection::Decoder)
            .map_err(refusal_to_open)?;
        codec.start().map_err(refusal_to_open)?;
        Ok(Codec {
            codec,
            kind,
            largest,
            _pictures: pictures,
        })
    }

    pub(super) fn kind(&self) -> CodecKind {
        self.kind
    }

    /// Whether it goes on with pictures of `size`, as it was configured to grow to.
    pub(super) fn fits(&self, size: PictureSize) -> bool {
        size.width <= self.largest.width && size.height <= self.largest.height
    }

    /// Hands `data`, which is `input`, to the codec, waiting up to `patience` for an input buffer;
    /// answers whether it was taken.
    pub(super) fn feed(
        &self,
        data: &[u8],
        input: Input,
        patience: Duration,
    ) -> Result<bool, DecoderError> {
        let mut buffer = match self.codec.dequeue_input_buffer(patience) {
            Ok(DequeuedInputBufferResult::Buffer(buffer)) => buffer,
            Ok(DequeuedInputBufferResult::TryAgainLater) => return Ok(false),
            Err(error) => return Err(refusal_to_decode(error)),
        };
        let room = buffer.buffer_mut();
        if room.len() < data.len() {
            return Err(DecoderError::BadData(format!(
                "a chunk of {} bytes, past the codec's {}",
                data.len(),
                room.len()
            )));
        }
        for (slot, byte) in room.iter_mut().zip(data) {
            slot.write(*byte);
        }
        let (number, flags) = match input {
            Input::Chunk(number) => (number, 0),
            Input::Configuration => (0, CODEC_CONFIG),
            Input::EndOfStream => (0, END_OF_STREAM),
        };
        self.codec
            .queue_input_buffer(buffer, 0, data.len(), number, flags)
            .map_err(refusal_to_decode)?;
        Ok(true)
    }

    /// What has come out of the codec, waiting up to `patience`: a picture is rendered into the
    /// stream where `wanted` says of its number, and dropped otherwise.
    pub(super) fn let_out(
        &self,
        patience: Duration,
        wanted: impl FnOnce(u64) -> bool,
    ) -> Result<Coming, DecoderError> {
        loop {
            let buffer = match self.codec.dequeue_output_buffer(patience) {
                Ok(DequeuedOutputBufferInfoResult::Buffer(buffer)) => buffer,
                Ok(DequeuedOutputBufferInfoResult::TryAgainLater) => return Ok(Coming::Nothing),
                // A new format or new buffers, said before the pictures they are of.
                Ok(_) => continue,
                Err(error) => return Err(refusal_to_decode(error)),
            };
            let info = *buffer.info();
            if info.flags() & END_OF_STREAM != 0 {
                self.codec
                    .release_output_buffer(buffer, false)
                    .map_err(refusal_to_decode)?;
                return Ok(Coming::EndOfStream);
            }
            let number = u64::try_from(info.presentation_time_us()).unwrap_or(0);
            let rendered = wanted(number);
            self.codec
                .release_output_buffer(buffer, rendered)
                .map_err(refusal_to_decode)?;
            return Ok(Coming::Picture { number, rendered });
        }
    }

    /// Drops every chunk and picture the codec holds, as for a seek or after its stream ended;
    /// it takes chunks again from a key frame.
    pub(super) fn flush(&self) -> Result<(), DecoderError> {
        self.codec.flush().map_err(refusal_to_decode)
    }
}

/// A codec of `codec`, whose MIME type is `mime`, of `choice`'s kind: the system's preferred
/// decoder, which is its hardware's where it has one, or its software decoder.
fn made(codec: VideoCodec, mime: &str, choice: CodecChoice) -> Result<MediaCodec, DecoderError> {
    let made = match choice {
        CodecChoice::Any | CodecChoice::Hardware => MediaCodec::from_decoder_type(mime),
        CodecChoice::Software => software_decoders(codec)
            .iter()
            .copied()
            .find_map(MediaCodec::from_codec_name),
    };
    made.ok_or_else(|| {
        DecoderError::Unsupported(format!(
            "the system made no {codec:?} decoder of the kind chosen"
        ))
    })
}

/// The codec a stream configured with `configuration` is of.
fn codec_of(configuration: &Configuration<'_>) -> VideoCodec {
    match configuration {
        Configuration::H264(_) => VideoCodec::H264,
        Configuration::Hevc(_) => VideoCodec::Hevc,
        Configuration::Vp9 { .. } => VideoCodec::Vp9,
        Configuration::Av1(_) => VideoCodec::Av1,
    }
}

/// Where `codec` runs, as its name says: the platform's own codecs run on the processor.
fn kind_of(codec: &MediaCodec) -> CodecKind {
    match codec.name() {
        Ok(name)
            if SOFTWARE_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix)) =>
        {
            CodecKind::Software
        }
        _ => CodecKind::Hardware,
    }
}

/// The larger of the stream's size and the largest the app says it reaches.
fn largest(size: PictureSize, largest_size: Option<PictureSize>) -> PictureSize {
    let largest_size = largest_size.unwrap_or(size);
    PictureSize {
        width: size.width.max(largest_size.width),
        height: size.height.max(largest_size.height),
    }
}

/// The codec's format: the codec MediaCodec names `mime`, of the stream's `size`, configured as
/// `configuration` says, able to grow to `largest`, and run as the pictures' purpose asks.
fn format_of(
    mime: &str,
    configuration: &Configuration<'_>,
    size: PictureSize,
    largest: PictureSize,
    purpose: DecodingPurpose,
) -> MediaFormat {
    let mut format = MediaFormat::new();
    format.set_str("mime", mime);
    format.set_i32("width", size.width as i32);
    format.set_i32("height", size.height as i32);
    format.set_i32("max-width", largest.width as i32);
    format.set_i32("max-height", largest.height as i32);
    match configuration {
        Configuration::H264(sets) => {
            format.set_buffer("csd-0", &with_start_codes(sets.sequence_sets()));
            format.set_buffer("csd-1", &with_start_codes(sets.picture_sets()));
        }
        Configuration::Hevc(sets) => format.set_buffer("csd-0", &with_start_codes(sets.units())),
        Configuration::Av1(sequence) => format.set_buffer("csd-0", &sequence.record()),
        Configuration::Vp9 { .. } => {}
    }
    // Android's `KEY_PRIORITY`: 0 is real time, 1 as fast as the codec can spare.
    let priority = match purpose {
        DecodingPurpose::Live | DecodingPurpose::Playback => 0,
        DecodingPurpose::Background => 1,
    };
    format.set_i32("priority", priority);
    if purpose == DecodingPurpose::Live {
        format.set_i32("low-latency", 1);
    }
    format
}

/// Why MediaCodec made no decoder, as the app can act on it.
fn refusal_to_open(error: MediaError) -> DecoderError {
    match error {
        MediaError::CodecErrorInsufficientResource => DecoderError::Busy,
        MediaError::ErrorUnsupported => {
            DecoderError::Unsupported(format!("the decoder does not take the stream ({error})"))
        }
        _ => DecoderError::Failed(format!("MediaCodec opened no decoder ({error})")),
    }
}

/// Why MediaCodec decoded no more, as the app can act on it: a stream it cannot follow, a codec
/// the system took back, or the decoder's failure.
fn refusal_to_decode(error: MediaError) -> DecoderError {
    match error {
        MediaError::ErrorMalformed => {
            DecoderError::BadData(format!("MediaCodec could not decode the chunk ({error})"))
        }
        MediaError::CodecErrorReclaimed => DecoderError::Lost,
        _ => DecoderError::Failed(format!("MediaCodec failed ({error})")),
    }
}
