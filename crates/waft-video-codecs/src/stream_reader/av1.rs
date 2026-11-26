//! An AV1 stream as a host's decoder reads it: its latest sequence header, from the file's
//! configuration record or from a chunk, so its description is whole once one has come; each
//! chunk whole. AV1 never reorders, so each chunk's picture stands after the one before.
//!
//! The invariant: the description changes only with a sequence header other than the one held.

use waft_embedder::video_decoders::{DecoderError, VideoCodec, VideoStreamSetup};

use super::{
    Configuration, Description, ReadChunk, StreamReader, another_codec, holds_no_parameter_set,
};
use crate::av1::{SequenceHeader, is_key_frame, obus};
use crate::presentation_order::Place;

pub(super) struct Av1Stream {
    /// The stream's latest sequence header, once one has come.
    sequence: Option<SequenceHeader>,
    /// How many chunks have been placed: the place of the next one's picture.
    chunks: i64,
}

impl Av1Stream {
    pub(super) fn of(record: Option<Vec<u8>>) -> Result<Av1Stream, DecoderError> {
        Ok(Av1Stream {
            sequence: sequence_of(record)?,
            chunks: 0,
        })
    }

    /// Goes on with `sequence`, where it is another than the one held; whether it did.
    fn take(&mut self, sequence: Option<SequenceHeader>) -> bool {
        let changed = sequence.is_some() && sequence != self.sequence;
        if changed {
            self.sequence = sequence;
        }
        changed
    }
}

impl StreamReader for Av1Stream {
    fn codec(&self) -> VideoCodec {
        VideoCodec::Av1
    }

    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::Av1(record) = setup else {
            return Err(another_codec(&setup));
        };
        let sequence = sequence_of(record)?;
        Ok(self.take(sequence))
    }

    /// A chunk with a sequence header begins a sequence, its picture shown after all before it.
    fn read<'a>(&mut self, chunk: &'a [u8]) -> Result<ReadChunk<'a>, DecoderError> {
        let units = obus(chunk).map_err(DecoderError::BadData)?;
        let sequence = SequenceHeader::among(&units).map_err(DecoderError::BadData)?;
        let begins_sequence = sequence.is_some();
        let changed = self.take(sequence);
        Ok(ReadChunk {
            changed,
            key_frame: is_key_frame(chunk),
            units: vec![chunk],
            is_parameter_set: holds_no_parameter_set,
            begins_sequence,
        })
    }

    fn place_of(&mut self, chunk: &ReadChunk<'_>) -> Result<Option<Place>, DecoderError> {
        let place = Place {
            count: self.chunks,
            begins_sequence: chunk.begins_sequence,
        };
        self.chunks += 1;
        Ok(Some(place))
    }

    fn description(&self) -> Result<Option<Description<'_>>, DecoderError> {
        let Some(sequence) = &self.sequence else {
            return Ok(None);
        };
        Ok(Some(Description {
            configuration: Configuration::Av1(sequence),
            facts: sequence.facts(),
        }))
    }
}

/// The sequence header the file's record (`av1C`) holds, where it gave one that holds one.
fn sequence_of(record: Option<Vec<u8>>) -> Result<Option<SequenceHeader>, DecoderError> {
    let Some(record) = record else {
        return Ok(None);
    };
    SequenceHeader::of_record(&record).map_err(DecoderError::BadData)
}
