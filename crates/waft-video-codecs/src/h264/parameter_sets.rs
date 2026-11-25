//! What an H.264 stream's parameter sets say: the sets themselves, which a system's decoder is
//! configured with, and what a host needs of the first of them before a picture is decoded: how
//! far the stream reorders its pictures and what their samples mean as colour.
//!
//! The invariant: a stream's reordering depth is never taken smaller than the stream may use,
//! since a picture let out too early cannot be taken back; where the stream does not say, it is
//! the most its level allows.

use h264_reader::avcc::AvcDecoderConfigurationRecord;
use h264_reader::nal::sps::SeqParameterSet;
use h264_reader::rbsp::{BitReader, decode_nal};
use waft_embedder::{ColorRange, PixelAspect, VideoColor};

use crate::StreamFacts;

const SEQUENCE_PARAMETER_SET: u8 = 7;
const PICTURE_PARAMETER_SET: u8 = 8;

/// H.264's Baseline profile, whose pictures never refer forward.
const BASELINE_PROFILE: u8 = 66;

/// The most pictures any level lets a stream hold back.
const MOST_PICTURES_HELD: usize = 16;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// What kind of unit `unit` is, as its first byte says; none for an empty one.
pub fn unit_type(unit: &[u8]) -> Option<u8> {
    unit.first().map(|header| header & 0x1F)
}

/// Whether `unit` is a parameter set, which goes to the decoder's format and not into a sample.
pub fn is_parameter_set(unit: &[u8]) -> bool {
    matches!(
        unit_type(unit),
        Some(SEQUENCE_PARAMETER_SET | PICTURE_PARAMETER_SET)
    )
}

/// A stream's parameter sets, each a unit without framing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterSets {
    sequence: Vec<Vec<u8>>,
    picture: Vec<Vec<u8>>,
}

impl ParameterSets {
    /// The sets a configuration record (`avcC`) holds, and the size of the length each unit of
    /// its stream follows; why not, for bytes that are no such record.
    pub fn of_record(record: &[u8]) -> Result<(ParameterSets, usize), String> {
        let parsed = AvcDecoderConfigurationRecord::try_from(record)
            .map_err(|error| format!("the configuration record: {error:?}"))?;
        let sequence = owned_sets(parsed.sequence_parameter_sets(), "sequence")?;
        let picture = owned_sets(parsed.picture_parameter_sets(), "picture")?;
        let length_size = usize::from(parsed.length_size_minus_one()) + 1;
        Ok((ParameterSets { sequence, picture }, length_size))
    }

    /// The sets among `units`.
    pub fn among(units: &[&[u8]]) -> ParameterSets {
        let of_type = |wanted: u8| {
            units
                .iter()
                .filter(|unit| unit_type(unit) == Some(wanted))
                .map(|unit| unit.to_vec())
                .collect()
        };
        ParameterSets {
            sequence: of_type(SEQUENCE_PARAMETER_SET),
            picture: of_type(PICTURE_PARAMETER_SET),
        }
    }

    /// These sets with each kind `newer` holds in place of this one's: a stream sends a kind
    /// again whole when it changes.
    pub fn replaced_by(&self, newer: ParameterSets) -> ParameterSets {
        ParameterSets {
            sequence: if newer.sequence.is_empty() {
                self.sequence.clone()
            } else {
                newer.sequence
            },
            picture: if newer.picture.is_empty() {
                self.picture.clone()
            } else {
                newer.picture
            },
        }
    }

    /// Whether a decoder's format can be made of them: at least one of each kind.
    pub fn are_complete(&self) -> bool {
        !self.sequence.is_empty() && !self.picture.is_empty()
    }

    /// The sets, sequence sets first, as VideoToolbox is handed them.
    pub fn units(&self) -> impl Iterator<Item = &[u8]> {
        self.sequence_sets().chain(self.picture_sets())
    }

    pub fn sequence_sets(&self) -> impl Iterator<Item = &[u8]> {
        self.sequence.iter().map(Vec::as_slice)
    }

    pub fn picture_sets(&self) -> impl Iterator<Item = &[u8]> {
        self.picture.iter().map(Vec::as_slice)
    }

    /// What the first sequence set says of the stream; why not, for one that cannot be read.
    pub fn facts(&self) -> Result<StreamFacts, String> {
        let first = self
            .sequence
            .first()
            .ok_or("the stream has no sequence parameter set")?;
        facts_of_sequence_set(first)
    }
}

/// The record's sets of one `kind`, each copied out; why not, for one the record cuts short.
fn owned_sets<'a, Refusal: std::fmt::Debug>(
    sets: impl Iterator<Item = Result<&'a [u8], Refusal>>,
    kind: &str,
) -> Result<Vec<Vec<u8>>, String> {
    sets.map(|set| match set {
        Ok(set) => Ok(set.to_vec()),
        Err(refusal) => Err(format!("the record's {kind} parameter sets: {refusal:?}")),
    })
    .collect()
}

/// What the sequence parameter set `unit` says of the stream; why not, for one that cannot be read.
fn facts_of_sequence_set(unit: &[u8]) -> Result<StreamFacts, String> {
    let payload = decode_nal(unit).map_err(|error| format!("a parameter set: {error}"))?;
    let set = SeqParameterSet::from_bits(BitReader::new(&payload[..]))
        .map_err(|error| format!("the sequence parameter set: {error:?}"))?;
    let usability = set.vui_parameters.as_ref();
    let said_depth = usability
        .and_then(|usability| usability.bitstream_restrictions.as_ref())
        .map(|restrictions| restrictions.max_num_reorder_frames as usize);
    let signal = usability.and_then(|usability| usability.video_signal_type.as_ref());
    let described = signal.and_then(|signal| signal.colour_description.as_ref());
    let said = |code: u8| (code != UNSPECIFIED).then_some(code);
    Ok(StreamFacts {
        reorder_depth: said_depth.unwrap_or_else(|| most_depth_allowed(&set)),
        color: VideoColor {
            primaries: described.and_then(|color| said(color.colour_primaries)),
            transfer: described.and_then(|color| said(color.transfer_characteristics)),
            matrix: described.and_then(|color| said(color.matrix_coefficients)),
            range: signal.map(|signal| {
                if signal.video_full_range_flag {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                }
            }),
            chroma_location: None,
        },
        pixel_aspect: usability
            .and_then(|usability| usability.aspect_ratio_info.as_ref())
            .and_then(|aspect| aspect.get())
            .map(|(width, height)| PixelAspect {
                width: u32::from(width),
                height: u32::from(height),
            }),
    })
}

/// The deepest a stream of `set`'s profile, level and size may reorder where it does not say:
/// none in Baseline, and elsewhere as many pictures as its level's buffer holds of its size
/// (H.264's table A-1, `MaxDpbMbs`).
fn most_depth_allowed(set: &SeqParameterSet) -> usize {
    if u8::from(set.profile_idc) == BASELINE_PROFILE {
        return 0;
    }
    let buffer_macroblocks: usize = match set.level_idc {
        ..=10 => 396,
        11 => 900,
        12..=20 => 2376,
        21 => 4752,
        22 | 30 => 8100,
        31 => 18000,
        32 => 20480,
        40 | 41 => 32768,
        42 => 34816,
        50 => 110_400,
        51 | 52 => 184_320,
        _ => 696_320,
    };
    let picture_macroblocks = set
        .pixel_dimensions()
        .map(|(width, height)| (width.div_ceil(16) * height.div_ceil(16)) as usize)
        .unwrap_or(1)
        .max(1);
    (buffer_macroblocks / picture_macroblocks).min(MOST_PICTURES_HELD)
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::{CLIP, record_of};
    use super::*;
    use crate::framing::units_after_start_codes;

    #[test]
    fn a_streams_sets_are_found_among_its_units() {
        let units = units_after_start_codes(CLIP);
        let sets = ParameterSets::among(&units);
        assert!(sets.are_complete());
        assert_eq!(sets.units().count(), 2);
        assert!(sets.units().all(is_parameter_set));
    }

    #[test]
    fn a_stream_says_how_far_it_reorders() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        assert_eq!(sets.facts().unwrap().reorder_depth, 2);
    }

    #[test]
    fn a_kind_sent_again_takes_the_place_of_the_one_held() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let other_picture_set = ParameterSets {
            sequence: Vec::new(),
            picture: vec![vec![PICTURE_PARAMETER_SET, 1]],
        };
        let replaced = sets.replaced_by(other_picture_set);
        assert_eq!(replaced.sequence, sets.sequence);
        assert_eq!(replaced.picture, [vec![PICTURE_PARAMETER_SET, 1]]);
        assert_eq!(sets.replaced_by(ParameterSets::default()), sets);
    }

    #[test]
    fn a_record_holds_the_sets_and_the_size_of_a_length() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let record = record_of(&sets);
        assert_eq!(ParameterSets::of_record(&record), Ok((sets, 4)));
        assert!(ParameterSets::of_record(&[1, 2, 3]).is_err());
    }
}
