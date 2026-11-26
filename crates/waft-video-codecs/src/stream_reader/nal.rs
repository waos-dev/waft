//! An H.264 or HEVC stream as a host's decoder reads it: its units after start codes or lengths,
//! as its setup says, and its parameter sets, wherever they came from, its setup or its chunks.
//! The two codecs are read alike; what differs (their units' types, their sets, how a picture's
//! order is counted) is each codec's module's, reached through [`NalCodec`].
//!
//! The invariant: the sets held are those the stream gave last of each kind, and only a whole
//! set of them, one other than the set held, changes the stream's description.

use std::marker::PhantomData;

use waft_embedder::video_decoders::{DecoderError, NalFormat, VideoCodec, VideoStreamSetup};

use super::{Configuration, Description, ReadChunk, StreamReader, another_codec};
use crate::StreamFacts;
use crate::framing::Framing;
use crate::presentation_order::Place;
use crate::{h264, hevc};

/// What a reader needs of a codec whose chunks are units with parameter sets beside them.
pub(super) trait NalCodec: Send + 'static {
    type ParameterSets: Clone + Default + PartialEq + Send;
    type PictureOrder: Default + Send;

    const CODEC: VideoCodec;

    /// The framing of a setup of this codec; the setup back, where it is another codec's.
    fn format_of(setup: VideoStreamSetup) -> Result<NalFormat, VideoStreamSetup>;
    fn setup(format: NalFormat) -> Result<(Framing, Self::ParameterSets), String>;
    fn sets_among(units: &[&[u8]]) -> Self::ParameterSets;
    fn replaced_by(sets: &Self::ParameterSets, newer: Self::ParameterSets) -> Self::ParameterSets;
    fn are_complete(sets: &Self::ParameterSets) -> bool;
    fn facts(sets: &Self::ParameterSets) -> Result<StreamFacts, String>;
    fn configuration(sets: &Self::ParameterSets) -> Configuration<'_>;
    fn is_parameter_set(unit: &[u8]) -> bool;
    fn is_key_frame(units: &[&[u8]]) -> bool;
    fn adopt(order: &mut Self::PictureOrder, sets: &Self::ParameterSets) -> Result<(), String>;
    fn place_of(order: &mut Self::PictureOrder, units: &[&[u8]]) -> Result<Option<Place>, String>;
}

pub(super) struct H264;

impl NalCodec for H264 {
    type ParameterSets = h264::ParameterSets;
    type PictureOrder = h264::PictureOrder;

    const CODEC: VideoCodec = VideoCodec::H264;

    fn format_of(setup: VideoStreamSetup) -> Result<NalFormat, VideoStreamSetup> {
        match setup {
            VideoStreamSetup::H264(format) => Ok(format),
            other => Err(other),
        }
    }

    fn setup(format: NalFormat) -> Result<(Framing, h264::ParameterSets), String> {
        h264::setup(format)
    }

    fn sets_among(units: &[&[u8]]) -> h264::ParameterSets {
        h264::ParameterSets::among(units)
    }

    fn replaced_by(sets: &h264::ParameterSets, newer: h264::ParameterSets) -> h264::ParameterSets {
        sets.replaced_by(newer)
    }

    fn are_complete(sets: &h264::ParameterSets) -> bool {
        sets.are_complete()
    }

    fn facts(sets: &h264::ParameterSets) -> Result<StreamFacts, String> {
        sets.facts()
    }

    fn configuration(sets: &h264::ParameterSets) -> Configuration<'_> {
        Configuration::H264(sets)
    }

    fn is_parameter_set(unit: &[u8]) -> bool {
        h264::is_parameter_set(unit)
    }

    fn is_key_frame(units: &[&[u8]]) -> bool {
        h264::is_key_frame(units)
    }

    fn adopt(order: &mut h264::PictureOrder, sets: &h264::ParameterSets) -> Result<(), String> {
        order.adopt(sets)
    }

    fn place_of(order: &mut h264::PictureOrder, units: &[&[u8]]) -> Result<Option<Place>, String> {
        order.place_of(units)
    }
}

pub(super) struct Hevc;

impl NalCodec for Hevc {
    type ParameterSets = hevc::ParameterSets;
    type PictureOrder = hevc::PictureOrder;

    const CODEC: VideoCodec = VideoCodec::Hevc;

    fn format_of(setup: VideoStreamSetup) -> Result<NalFormat, VideoStreamSetup> {
        match setup {
            VideoStreamSetup::Hevc(format) => Ok(format),
            other => Err(other),
        }
    }

    fn setup(format: NalFormat) -> Result<(Framing, hevc::ParameterSets), String> {
        hevc::setup(format)
    }

    fn sets_among(units: &[&[u8]]) -> hevc::ParameterSets {
        hevc::ParameterSets::among(units)
    }

    fn replaced_by(sets: &hevc::ParameterSets, newer: hevc::ParameterSets) -> hevc::ParameterSets {
        sets.replaced_by(newer)
    }

    fn are_complete(sets: &hevc::ParameterSets) -> bool {
        sets.are_complete()
    }

    fn facts(sets: &hevc::ParameterSets) -> Result<StreamFacts, String> {
        sets.facts()
    }

    fn configuration(sets: &hevc::ParameterSets) -> Configuration<'_> {
        Configuration::Hevc(sets)
    }

    fn is_parameter_set(unit: &[u8]) -> bool {
        hevc::is_parameter_set(unit)
    }

    fn is_key_frame(units: &[&[u8]]) -> bool {
        hevc::is_key_frame(units)
    }

    fn adopt(order: &mut hevc::PictureOrder, sets: &hevc::ParameterSets) -> Result<(), String> {
        order.adopt(sets)
    }

    fn place_of(order: &mut hevc::PictureOrder, units: &[&[u8]]) -> Result<Option<Place>, String> {
        order.place_of(units)
    }
}

/// An H.264 or HEVC stream, as far as it has come.
pub(super) struct NalStream<Codec: NalCodec> {
    framing: Framing,
    /// The stream's parameter sets, once a whole set of them has come.
    sets: Codec::ParameterSets,
    /// Where each picture stands in the order its stream is shown in, as the stream says.
    picture_order: Codec::PictureOrder,
    codec: PhantomData<Codec>,
}

impl<Codec: NalCodec> NalStream<Codec> {
    pub(super) fn of(format: NalFormat) -> Result<NalStream<Codec>, DecoderError> {
        let (framing, sets) = Codec::setup(format).map_err(DecoderError::BadData)?;
        let mut stream = NalStream {
            framing,
            sets: Codec::ParameterSets::default(),
            picture_order: Codec::PictureOrder::default(),
            codec: PhantomData,
        };
        stream.take_sets(sets)?;
        Ok(stream)
    }

    /// Goes on with `newer` in place of the sets of each kind it holds, where that makes a whole
    /// set other than the one held; whether it did.
    fn take_sets(&mut self, newer: Codec::ParameterSets) -> Result<bool, DecoderError> {
        let sets = Codec::replaced_by(&self.sets, newer);
        if !Codec::are_complete(&sets) || sets == self.sets {
            return Ok(false);
        }
        Codec::adopt(&mut self.picture_order, &sets).map_err(DecoderError::BadData)?;
        self.sets = sets;
        Ok(true)
    }
}

impl<Codec: NalCodec> StreamReader for NalStream<Codec> {
    fn codec(&self) -> VideoCodec {
        Codec::CODEC
    }

    fn configure(&mut self, setup: VideoStreamSetup) -> Result<bool, DecoderError> {
        let format = Codec::format_of(setup).map_err(|setup| another_codec(&setup))?;
        let (framing, sets) = Codec::setup(format).map_err(DecoderError::BadData)?;
        self.framing = framing;
        self.take_sets(sets)
    }

    /// The chunk's units, the parameter sets among them taken as the stream's.
    fn read<'a>(&mut self, chunk: &'a [u8]) -> Result<ReadChunk<'a>, DecoderError> {
        let units = self.framing.units(chunk).map_err(DecoderError::BadData)?;
        let changed = self.take_sets(Codec::sets_among(&units))?;
        Ok(ReadChunk {
            changed,
            key_frame: Codec::is_key_frame(&units),
            units,
            is_parameter_set: Codec::is_parameter_set,
            begins_sequence: false,
        })
    }

    /// Its picture placed by its first slice. A picture is counted before it is decoded, so one
    /// the system's decoder gives nothing for is counted all the same.
    fn place_of(&mut self, chunk: &ReadChunk<'_>) -> Result<Option<Place>, DecoderError> {
        Codec::place_of(&mut self.picture_order, &chunk.coded_units())
            .map_err(DecoderError::BadData)
    }

    fn description(&self) -> Result<Option<Description<'_>>, DecoderError> {
        if !Codec::are_complete(&self.sets) {
            return Ok(None);
        }
        Ok(Some(Description {
            configuration: Codec::configuration(&self.sets),
            facts: Codec::facts(&self.sets).map_err(DecoderError::BadData)?,
        }))
    }
}
