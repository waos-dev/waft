//! One H.264 decoder of the system's, as the NDK makes it: an `AMediaCodec` configured with the
//! stream's parameter sets, drawing its pictures into the window of an image stream of `PRIVATE`
//! pictures, as Android's own players draw into a surface. A picture comes out of the codec as an
//! output buffer, in the order pictures are shown, which the codec reads from the stream; it is let
//! into the stream by rendering it, and reaches what the codec was made with on the stream's own
//! thread, as an image whose memory is its `AHardwareBuffer`.
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
    CodecChoice, CodecKind, DecoderError, DecodingPurpose, PictureSize,
};

use super::stream::{ParameterSets, StreamFacts};
use crate::media::image_stream::{Delivery, ImageStream, StreamImage};

/// H.264's MIME type, as MediaCodec names the codec.
const H264: &str = "video/avc";

/// The system's software H.264 decoders, by the names Codec2 and the older OMX give them.
const SOFTWARE_DECODERS: [&str; 2] = ["c2.android.avc.decoder", "OMX.google.h264.decoder"];

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
    /// Parameter sets the stream goes on with.
    ParameterSets,
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
    /// A decoder of the stream `sets` begin, of the kind `options` chooses, its pictures handed to
    /// `delivered` on the stream's thread, in the order the codec renders them.
    pub(super) fn open(
        sets: &ParameterSets,
        facts: &StreamFacts,
        options: CodecOptions,
        delivered: impl Fn(StreamImage) + Send + 'static,
    ) -> Result<Codec, DecoderError> {
        let codec = made(options.choice)?;
        let kind = kind_of(&codec);
        if options.choice == CodecChoice::Hardware && kind == CodecKind::Software {
            return Err(DecoderError::Unsupported(
                "the system has no H.264 decoder on its video hardware".to_owned(),
            ));
        }
        let largest = largest(facts.size, options.largest_size);
        let pictures = ImageStream::new(
            [facts.size.width, facts.size.height],
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
        let format = format_of(sets, facts, largest, options.purpose);
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
            Input::ParameterSets => (0, CODEC_CONFIG),
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

/// A codec of `choice`'s kind: the system's preferred H.264 decoder, which is its hardware's where
/// it has one, or its software decoder.
fn made(choice: CodecChoice) -> Result<MediaCodec, DecoderError> {
    let codec = match choice {
        CodecChoice::Any | CodecChoice::Hardware => MediaCodec::from_decoder_type(H264),
        CodecChoice::Software => SOFTWARE_DECODERS
            .into_iter()
            .find_map(MediaCodec::from_codec_name),
    };
    codec.ok_or_else(|| {
        DecoderError::Unsupported("the system made no H.264 decoder of the kind chosen".to_owned())
    })
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

/// The codec's format: H.264 of the stream's size with its parameter sets, able to grow to
/// `largest`, and run as the pictures' purpose asks.
fn format_of(
    sets: &ParameterSets,
    facts: &StreamFacts,
    largest: PictureSize,
    purpose: DecodingPurpose,
) -> MediaFormat {
    let mut format = MediaFormat::new();
    format.set_str("mime", H264);
    format.set_i32("width", facts.size.width as i32);
    format.set_i32("height", facts.size.height as i32);
    format.set_i32("max-width", largest.width as i32);
    format.set_i32("max-height", largest.height as i32);
    format.set_buffer("csd-0", &sets.sequence_data());
    format.set_buffer("csd-1", &sets.picture_data());
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
