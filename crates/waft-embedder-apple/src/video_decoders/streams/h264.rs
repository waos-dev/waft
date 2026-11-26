//! An H.264 stream as VideoToolbox takes it: its parameter sets as its format, wherever they
//! came from, its setup or its chunks, and each chunk's other units after lengths of four bytes.

use waft_embedder::video_decoders::{DecoderError, NalFormat, VideoStreamSetup};
use waft_video_codecs::framing::{Framing, with_lengths};
use waft_video_codecs::h264::{self, ParameterSets, PictureOrder, is_parameter_set};

use super::{Description, ReadChunk, StreamReader, another_codec};
use crate::video_decoders::format::Format;

pub(super) struct H264Stream {
    framing: Framing,
    /// The stream's parameter sets, once a whole set of them has come.
    sets: ParameterSets,
    /// Where each picture stands in the order its stream is shown in, as the stream says.
    picture_order: PictureOrder,
}

impl H264Stream {
    pub(super) fn of(format: NalFormat) -> Result<H264Stream, DecoderError> {
        let (framing, sets) = h264::setup(format).map_err(DecoderError::BadData)?;
        let mut stream = H264Stream {
            framing,
            sets: ParameterSets::default(),
            picture_order: PictureOrder::default(),
        };
        stream.take_sets(sets)?;
        Ok(stream)
    }

    /// Goes on with `newer` in place of the sets of each kind it holds, where that makes a whole
    /// set other than the one held; whether it did.
    fn take_sets(&mut self, newer: ParameterSets) -> Result<bool, DecoderError> {
        let sets = self.sets.replaced_by(newer);
        if !sets.are_complete() || sets == self.sets {
            return Ok(false);
        }
        self.picture_order
            .adopt(&sets)
            .map_err(DecoderError::BadData)?;
        self.sets = sets;
        Ok(true)
    }
}

impl StreamReader for H264Stream {
    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::H264(format) = setup else {
            return Err(another_codec(&setup));
        };
        let (framing, sets) = h264::setup(format).map_err(DecoderError::BadData)?;
        self.framing = framing;
        self.take_sets(sets)
    }

    /// The chunk's units but its parameter sets, which go to the format, each after its length,
    /// its picture placed by its first slice. A picture is counted before it is decoded, so one
    /// VideoToolbox gives nothing for is counted all the same.
    fn read(&mut self, chunk: &[u8]) -> Result<ReadChunk, DecoderError> {
        let units = self.framing.units(chunk).map_err(DecoderError::BadData)?;
        let changed = self.take_sets(ParameterSets::among(&units))?;
        let coded: Vec<&[u8]> = units
            .into_iter()
            .filter(|unit| !is_parameter_set(unit))
            .collect();
        let place = self
            .picture_order
            .place_of(&coded)
            .map_err(DecoderError::BadData)?;
        Ok(ReadChunk {
            changed,
            sample: with_lengths(coded),
            place,
        })
    }

    fn description(&self) -> Result<Option<Description>, DecoderError> {
        if !self.sets.are_complete() {
            return Ok(None);
        }
        Ok(Some(Description {
            format: Format::h264(&self.sets)?,
            facts: self.sets.facts().map_err(DecoderError::BadData)?,
        }))
    }
}
