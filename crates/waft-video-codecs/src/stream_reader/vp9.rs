//! A VP9 stream as a host's decoder reads it: the file's configuration record, where it gave one,
//! and what the stream's last key frame said, so its description is whole from its first key
//! frame on; each chunk whole, a superframe and all. VP9 never reorders, so each chunk's picture
//! stands after the one before.
//!
//! The invariant: the description changes only with a key frame that says otherwise, or a record
//! that differs once a key frame has come.

use waft_embedder::video_decoders::{DecoderError, VideoCodec, VideoStreamSetup};

use super::{
    Configuration, Description, ReadChunk, StreamReader, another_codec, holds_no_parameter_set,
};
use crate::presentation_order::Place;
use crate::vp9::{KeyFrame, Record};

pub(super) struct Vp9Stream {
    /// The configuration record the app's file gave, where it gave one.
    record: Option<Record>,
    /// What the stream's last key frame said, once one has come.
    key: Option<KeyFrame>,
    /// How many chunks have been placed: the place of the next one's picture.
    chunks: i64,
}

impl Vp9Stream {
    pub(super) fn of(record: Option<Vec<u8>>) -> Result<Vp9Stream, DecoderError> {
        Ok(Vp9Stream {
            record: record_of(record)?,
            key: None,
            chunks: 0,
        })
    }
}

impl StreamReader for Vp9Stream {
    fn codec(&self) -> VideoCodec {
        VideoCodec::Vp9
    }

    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let VideoStreamSetup::Vp9(record) = setup else {
            return Err(another_codec(&setup));
        };
        let record = record_of(record)?;
        let changed = record != self.record && self.key.is_some();
        self.record = record;
        Ok(changed)
    }

    fn read<'a>(&mut self, chunk: &'a [u8]) -> Result<ReadChunk<'a>, DecoderError> {
        let key = KeyFrame::of_chunk(chunk).map_err(DecoderError::BadData)?;
        let changed = key.is_some() && key != self.key;
        if key.is_some() {
            self.key = key;
        }
        Ok(ReadChunk {
            changed,
            key_frame: key.is_some(),
            units: vec![chunk],
            is_parameter_set: holds_no_parameter_set,
            begins_sequence: key.is_some(),
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
        let Some(key) = &self.key else {
            return Ok(None);
        };
        let record = match self.record {
            Some(record) => record,
            None => Record::of_key_frame(key).map_err(DecoderError::BadData)?,
        };
        Ok(Some(Description {
            configuration: Configuration::Vp9 {
                record,
                size: key.size,
            },
            facts: key.facts(self.record.as_ref()),
        }))
    }
}

/// The record of `bytes`, the contents of the file's `vpcC` box, where it gave one.
fn record_of(bytes: Option<Vec<u8>>) -> Result<Option<Record>, DecoderError> {
    bytes
        .map(|bytes| Record::parse(&bytes).map_err(DecoderError::BadData))
        .transpose()
}
