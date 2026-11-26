//! A stream as a host's decoder reads it, the same for every codec: what its setup and its chunks
//! say of it, kept as they come. Every host's decoder asks the same of a stream before the
//! system's decoder sees it, so the asking is written once here, over the codecs' own rules:
//! - how its chunks frame their units, and the units of each ([`ReadChunk`]);
//! - what the system's decoder is configured with ([`Configuration`]): H.264's and HEVC's parameter
//!   sets, VP9's configuration record, AV1's sequence header, wherever they came from, its setup
//!   or its chunks, and whether a chunk changed it;
//! - which chunk decodes with nothing before it;
//! - where a picture stands in the order its stream is shown in ([`StreamReader::place_of`]), for
//!   a system whose decoder leaves that to its caller;
//! - what the stream says of its pictures ([`StreamFacts`]).
//!
//! H.264 and HEVC are read alike, their units after start codes or lengths and their parameter
//! sets beside them (`nal`); VP9 and AV1 each a chunk whole, configured from a record or a header
//! (`vp9`, `av1`).
//!
//! The invariant: a reader's description is of the stream as far as its setup and its chunks
//! have said, and a chunk or a setup changed it exactly when a decoder configured with the one
//! before may no longer decode the stream.

mod av1;
mod nal;
mod vp9;

use waft_embedder::video_decoders::{DecoderError, PictureSize, VideoCodec, VideoStreamSetup};

use crate::StreamFacts;
use crate::presentation_order::Place;

/// A stream, as far as its setup and its chunks have said.
pub trait StreamReader: Send {
    fn codec(&self) -> VideoCodec;

    /// The stream's setup from the next chunk on; whether it changed the description.
    /// `Unsupported` for a setup of another codec than the stream's.
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError>;

    /// What `chunk` holds, the stream taking what it says of itself; `BadData` for a chunk whose
    /// units cannot be told apart.
    fn read<'a>(&mut self, chunk: &'a [u8]) -> Result<ReadChunk<'a>, DecoderError>;

    /// Where the picture of `chunk`, read last, stands among those of its stream; none for a
    /// chunk with no picture. Asked of every chunk read, in order, or of none: a count is told
    /// from the picture before it.
    fn place_of(&mut self, chunk: &ReadChunk<'_>) -> Result<Option<Place>, DecoderError>;

    /// The stream's description; none until enough of it has come.
    fn description(&self) -> Result<Option<Description<'_>>, DecoderError>;
}

/// The reader of the stream `setup` begins; `Unsupported` for VP8, which is not read here.
pub fn reader_for(setup: VideoStreamSetup) -> Result<Box<dyn StreamReader>, DecoderError> {
    match setup {
        VideoStreamSetup::H264(format) => Ok(Box::new(nal::NalStream::<nal::H264>::of(format)?)),
        VideoStreamSetup::Hevc(format) => Ok(Box::new(nal::NalStream::<nal::Hevc>::of(format)?)),
        VideoStreamSetup::Vp9(record) => Ok(Box::new(vp9::Vp9Stream::of(record)?)),
        VideoStreamSetup::Av1(record) => Ok(Box::new(av1::Av1Stream::of(record)?)),
        VideoStreamSetup::Vp8 => Err(DecoderError::Unsupported(
            "a VP8 stream is not read here".to_owned(),
        )),
    }
}

/// What a system's decoder is made for: what it is configured with, and what the stream says of
/// its pictures.
pub struct Description<'a> {
    pub configuration: Configuration<'a>,
    pub facts: StreamFacts,
}

/// What a system's decoder is configured with, a codec's own each.
pub enum Configuration<'a> {
    H264(&'a crate::h264::ParameterSets),
    Hevc(&'a crate::hevc::ParameterSets),
    /// The file's configuration record, or one made of what the stream's key frame says where the
    /// file gave none, and the size of the key frame's pictures, which no record says.
    Vp9 {
        record: crate::vp9::Record,
        size: PictureSize,
    },
    /// The stream's latest sequence header, from the file's record or from a chunk.
    Av1(&'a crate::av1::SequenceHeader),
}

/// What one chunk holds.
pub struct ReadChunk<'a> {
    /// Whether the chunk changed the stream's description.
    pub changed: bool,
    /// Whether it decodes with nothing before it, as its units say.
    pub key_frame: bool,
    units: Vec<&'a [u8]>,
    is_parameter_set: fn(&[u8]) -> bool,
    /// Whether a VP9 or AV1 chunk begins a sequence, its picture shown after all before it.
    begins_sequence: bool,
}

impl<'a> ReadChunk<'a> {
    /// Its units in order: an H.264 or HEVC chunk's units, each without its framing, or a VP9 or
    /// AV1 chunk whole, as one, which a system's decoder takes so.
    pub fn units(&self) -> &[&'a [u8]] {
        &self.units
    }

    /// Its units but the parameter sets among them, which a decoder configured with them is not
    /// handed again: what codes its picture.
    pub fn coded_units(&self) -> Vec<&'a [u8]> {
        self.units
            .iter()
            .copied()
            .filter(|unit| !(self.is_parameter_set)(unit))
            .collect()
    }
}

/// Why a reader takes no setup of another codec than the one its stream began with.
fn another_codec(setup: &VideoStreamSetup) -> DecoderError {
    DecoderError::Unsupported(format!(
        "a stream goes on in its own codec, not {:?}",
        setup.codec()
    ))
}

/// That a VP9 or AV1 chunk holds no parameter set: it is handed over whole.
fn holds_no_parameter_set(_: &[u8]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use waft_embedder::video_decoders::NalFormat;

    use super::*;
    use crate::framing::{units_after_start_codes, with_start_codes};
    use crate::{av1, h264, hevc, vp9};

    #[test]
    fn an_h264_stream_is_described_once_its_sets_have_come() {
        let mut reader = reader_for(VideoStreamSetup::H264(NalFormat::StartCodes(None))).unwrap();
        assert!(reader.description().unwrap().is_none());
        let pictures = h264::test_clip::pictures_of_the_clip();
        let first = with_start_codes(pictures[0].iter().copied());
        let read = reader.read(&first).unwrap();
        assert!(read.changed && read.key_frame);
        assert!(read.coded_units().len() < read.units().len(), "its sets");
        assert!(
            read.coded_units()
                .iter()
                .all(|unit| !h264::is_parameter_set(unit))
        );
        let place = reader.place_of(&read).unwrap().expect("a picture");
        assert!(place.begins_sequence);
        let description = reader.description().unwrap().expect("whole");
        assert!(matches!(description.configuration, Configuration::H264(_)));
        assert_eq!(
            description.facts.size,
            Some(PictureSize {
                width: 320,
                height: 240
            })
        );
        let second = with_start_codes(pictures[1].iter().copied());
        let read = reader.read(&second).unwrap();
        assert!(!read.changed && !read.key_frame);
    }

    #[test]
    fn an_hevc_stream_with_its_record_is_described_from_the_start() {
        let sets = hevc::ParameterSets::among(&units_after_start_codes(hevc::test_clip::CLIP));
        let record = hevc::test_clip::record_of(&sets);
        let reader = reader_for(VideoStreamSetup::Hevc(NalFormat::LengthPrefixed(record))).unwrap();
        assert_eq!(reader.codec(), VideoCodec::Hevc);
        let description = reader.description().unwrap().expect("whole");
        assert!(matches!(description.configuration, Configuration::Hevc(held) if *held == sets));
    }

    #[test]
    fn a_vp9_stream_is_described_from_its_first_key_frame_and_never_reorders() {
        let mut reader = reader_for(VideoStreamSetup::Vp9(None)).unwrap();
        assert!(reader.description().unwrap().is_none());
        let chunks = vp9::test_clip::chunks_of_the_clip();
        let places: Vec<Place> = chunks
            .iter()
            .map(|chunk| {
                let read = reader.read(chunk).unwrap();
                assert_eq!(read.units(), [*chunk], "a chunk is handed over whole");
                reader.place_of(&read).unwrap().expect("a picture")
            })
            .collect();
        let counts: Vec<i64> = places.iter().map(|place| place.count).collect();
        assert_eq!(counts, (0..12).collect::<Vec<i64>>());
        assert!(places[0].begins_sequence && !places[1].begins_sequence);
        let description = reader.description().unwrap().expect("whole");
        assert!(matches!(
            description.configuration,
            Configuration::Vp9 {
                size: PictureSize {
                    width: 320,
                    height: 240
                },
                ..
            }
        ));
    }

    #[test]
    fn an_av1_stream_is_described_from_its_first_sequence_header() {
        let mut reader = reader_for(VideoStreamSetup::Av1(None)).unwrap();
        let chunks = av1::test_clip::chunks_of_the_clip();
        let first = reader.read(chunks[0]).unwrap();
        assert!(first.changed && first.key_frame);
        let description = reader.description().unwrap().expect("whole");
        assert!(matches!(description.configuration, Configuration::Av1(_)));
        assert_eq!(description.facts.bit_depth, 8);
        let second = reader.read(chunks[1]).unwrap();
        assert!(!second.changed && !second.key_frame);
    }

    #[test]
    fn a_stream_goes_on_in_its_own_codec() {
        let mut reader = reader_for(VideoStreamSetup::Av1(None)).unwrap();
        assert!(matches!(
            reader.configure(VideoStreamSetup::Vp9(None)),
            Err(DecoderError::Unsupported(_))
        ));
        assert!(matches!(
            reader_for(VideoStreamSetup::Vp8),
            Err(DecoderError::Unsupported(_))
        ));
    }
}
