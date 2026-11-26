//! What a decoder reads of its stream before VideoToolbox decodes it, a codec's way each: the
//! stream's description, once it has come (its format, and what it says of its pictures), and of
//! each chunk the sample VideoToolbox takes and where its picture stands in the order its stream
//! is shown in. The codec's rules are `waft-video-codecs`'; what is here is how VideoToolbox
//! takes them.
//!
//! The invariant: a reader's description is of the stream as far as its setup and its chunks
//! have said, and it says a chunk changed it exactly when the format a session was made for may
//! no longer be the stream's.

mod h264;
mod hevc;

use waft_embedder::video_decoders::{DecoderError, VideoStreamSetup};
use waft_video_codecs::StreamFacts;
use waft_video_codecs::presentation_order::Place;

use super::format::Format;

/// A stream as one codec's reader reads it.
pub(super) trait StreamReader: Send {
    /// The stream's setup from the next chunk on; whether it changed the description.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError>;

    /// What `chunk` is to VideoToolbox, the stream taking what the chunk says of it.
    fn read(&mut self, chunk: &[u8]) -> Result<ReadChunk, DecoderError>;

    /// The stream's description; none until enough of it has come.
    fn description(&self) -> Result<Option<Description>, DecoderError>;
}

/// What a session is made for: a stream's format, and what it says of its pictures.
pub(super) struct Description {
    pub(super) format: Format,
    pub(super) facts: StreamFacts,
}

/// What a chunk is to VideoToolbox.
pub(super) struct ReadChunk {
    /// Whether the chunk changed the stream's description.
    pub(super) changed: bool,
    /// The sample VideoToolbox decodes, framed as the stream's format says.
    pub(super) sample: Vec<u8>,
    /// Where its picture stands among those of its stream; none for a chunk with no picture.
    pub(super) place: Option<Place>,
}

/// The reader of the stream `setup` begins.
pub(super) fn reader_for(setup: VideoStreamSetup) -> Result<Box<dyn StreamReader>, DecoderError> {
    match setup {
        VideoStreamSetup::H264(format) => Ok(Box::new(h264::H264Stream::of(format)?)),
        VideoStreamSetup::Hevc(format) => Ok(Box::new(hevc::HevcStream::of(format)?)),
        other => Err(DecoderError::Unsupported(format!(
            "this system's decoders are handed H.264 and HEVC alone so far, not {:?}",
            other.codec()
        ))),
    }
}

/// Why a reader takes no setup of another codec than the one its stream began with.
fn another_codec(setup: &VideoStreamSetup) -> DecoderError {
    DecoderError::Unsupported(format!(
        "a stream goes on in its own codec, not {:?}",
        setup.codec()
    ))
}
