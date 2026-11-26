//! What an HEVC stream's parameter sets say: the sets themselves, which a system's decoder is
//! configured with (VideoToolbox's format, MediaCodec's codec-specific data, each video set
//! first, then the sequence sets, then the picture sets), and what a host needs of the first
//! sequence set before a picture is decoded: how far the stream reorders its pictures, what
//! their samples mean as colour, their size and profile.
//!
//! The invariant: a stream's reordering depth is the one its sequence set gives for all its
//! sub-layers, never a smaller one, since a picture let out too early cannot be taken back.

use std::io::Cursor;

use scuffle_h265::{
    AspectRatioIdc, AspectRatioInfo, HEVCDecoderConfigurationRecord, NALUnitType, SpsNALUnit,
    VideoSignalType,
};
use waft_embedder::video_decoders::PictureSize;
use waft_embedder::{ChromaAlignment, ChromaLocation, ColorRange, PixelAspect, VideoColor};

use super::unit_type;
use crate::StreamFacts;

const VIDEO_PARAMETER_SET: u8 = 32;
const SEQUENCE_PARAMETER_SET: u8 = 33;
const PICTURE_PARAMETER_SET: u8 = 34;

/// H.273's code for a value the stream leaves unsaid.
const UNSPECIFIED: u8 = 2;

/// Whether `unit` is a parameter set, which goes to the decoder's format and not into a sample.
pub fn is_parameter_set(unit: &[u8]) -> bool {
    matches!(
        unit_type(unit),
        Some(VIDEO_PARAMETER_SET | SEQUENCE_PARAMETER_SET | PICTURE_PARAMETER_SET)
    )
}

/// A stream's parameter sets, each a unit without framing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterSets {
    video: Vec<Vec<u8>>,
    sequence: Vec<Vec<u8>>,
    picture: Vec<Vec<u8>>,
}

impl ParameterSets {
    /// The sets a configuration record (`hvcC`) holds, and the size of the length each unit of
    /// its stream follows; why not, for bytes that are no such record. The record's SEI units,
    /// which some files carry beside the sets, are not sets and are left out.
    pub fn of_record(record: &[u8]) -> Result<(ParameterSets, usize), String> {
        let parsed = HEVCDecoderConfigurationRecord::demux(Cursor::new(record))
            .map_err(|error| format!("the configuration record: {error}"))?;
        let of_type = |wanted: NALUnitType| -> Vec<Vec<u8>> {
            parsed
                .arrays
                .iter()
                .filter(|array| array.nal_unit_type == wanted)
                .flat_map(|array| array.nalus.iter().map(|unit| unit.to_vec()))
                .collect()
        };
        let sets = ParameterSets {
            video: of_type(NALUnitType::VpsNut),
            sequence: of_type(NALUnitType::SpsNut),
            picture: of_type(NALUnitType::PpsNut),
        };
        let length_size = usize::from(parsed.length_size_minus_one) + 1;
        Ok((sets, length_size))
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
            video: of_type(VIDEO_PARAMETER_SET),
            sequence: of_type(SEQUENCE_PARAMETER_SET),
            picture: of_type(PICTURE_PARAMETER_SET),
        }
    }

    /// These sets with each kind `newer` holds in place of this one's: a stream sends a kind
    /// again whole when it changes.
    pub fn replaced_by(&self, newer: ParameterSets) -> ParameterSets {
        let kept = |held: &Vec<Vec<u8>>, newer: Vec<Vec<u8>>| {
            if newer.is_empty() {
                held.clone()
            } else {
                newer
            }
        };
        ParameterSets {
            video: kept(&self.video, newer.video),
            sequence: kept(&self.sequence, newer.sequence),
            picture: kept(&self.picture, newer.picture),
        }
    }

    /// Whether a decoder's format can be made of them: at least one of each kind.
    pub fn are_complete(&self) -> bool {
        !self.video.is_empty() && !self.sequence.is_empty() && !self.picture.is_empty()
    }

    /// The sets, video sets first, then sequence sets, then picture sets, as VideoToolbox and
    /// MediaCodec are handed them.
    pub fn units(&self) -> impl Iterator<Item = &[u8]> {
        self.video_sets()
            .chain(self.sequence_sets())
            .chain(self.picture_sets())
    }

    pub fn video_sets(&self) -> impl Iterator<Item = &[u8]> {
        self.video.iter().map(Vec::as_slice)
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

/// What the sequence parameter set `unit` says of the stream; why not, for one that cannot be read.
fn facts_of_sequence_set(unit: &[u8]) -> Result<StreamFacts, String> {
    let set = SpsNALUnit::parse(unit)
        .map_err(|error| format!("the sequence parameter set: {error}"))?
        .rbsp;
    let usability = set.vui_parameters.as_ref();
    let signal = usability
        .map(|usability| &usability.video_signal_type)
        // The reader gives a signal the stream leaves out as the standard infers it, so one that
        // reads as that inference says nothing.
        .filter(|signal| **signal != VideoSignalType::default());
    let said = |code: u8| (code != UNSPECIFIED).then_some(code);
    let profile = &set.profile_tier_level.general_profile;
    Ok(StreamFacts {
        reorder_depth: set
            .sub_layer_ordering_info
            .sps_max_num_reorder_pics
            .last()
            .map_or(0, |depth| *depth as usize),
        color: VideoColor {
            primaries: signal.and_then(|signal| said(signal.colour_primaries)),
            transfer: signal.and_then(|signal| said(signal.transfer_characteristics)),
            matrix: signal.and_then(|signal| said(signal.matrix_coeffs)),
            range: signal.map(|signal| {
                if signal.video_full_range_flag {
                    ColorRange::Full
                } else {
                    ColorRange::Limited
                }
            }),
            chroma_location: usability
                .and_then(|usability| usability.chroma_loc_info.as_ref())
                .and_then(|location| chroma_location(location.top_field)),
        },
        pixel_aspect: usability.and_then(|usability| pixel_aspect(&usability.aspect_ratio_info)),
        size: Some(PictureSize {
            width: set.cropped_width() as u32,
            height: set.cropped_height() as u32,
        }),
        profile: u32::from(profile.profile_idc),
        level: profile.level_idc.map(u32::from),
    })
}

/// The shape of a pixel a stream's aspect ratio gives: its own, or one of H.273's table 8;
/// none where it is unsaid or reserved.
fn pixel_aspect(info: &AspectRatioInfo) -> Option<PixelAspect> {
    let (width, height) = match info {
        AspectRatioInfo::ExtendedSar {
            sar_width,
            sar_height,
        } => (u32::from(*sar_width), u32::from(*sar_height)),
        AspectRatioInfo::Predefined(AspectRatioIdc(code)) => match code {
            1 => (1, 1),
            2 => (12, 11),
            3 => (10, 11),
            4 => (16, 11),
            5 => (40, 33),
            6 => (24, 11),
            7 => (20, 11),
            8 => (32, 11),
            9 => (80, 33),
            10 => (18, 11),
            11 => (15, 11),
            12 => (64, 33),
            13 => (160, 99),
            14 => (4, 3),
            15 => (3, 2),
            16 => (2, 1),
            _ => return None,
        },
    };
    (width != 0 && height != 0).then_some(PixelAspect { width, height })
}

/// Where a stream's colour samples sit, as H.273's chroma sample location type says; none for
/// the two below their brightness samples, which no alignment here names.
fn chroma_location(location_type: u64) -> Option<ChromaLocation> {
    use ChromaAlignment::{Centered, Cosited};
    let (horizontal, vertical) = match location_type {
        0 => (Cosited, Centered),
        1 => (Centered, Centered),
        2 => (Cosited, Cosited),
        3 => (Centered, Cosited),
        _ => return None,
    };
    Some(ChromaLocation {
        horizontal,
        vertical,
    })
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::{CLIP, MAIN_10_CLIP, record_of};
    use super::*;
    use crate::framing::units_after_start_codes;

    #[test]
    fn a_streams_sets_are_found_among_its_units() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        assert!(sets.are_complete());
        assert_eq!(sets.units().count(), 3);
        assert!(sets.units().all(is_parameter_set));
        let kinds: Vec<Option<u8>> = sets.units().map(unit_type).collect();
        assert_eq!(
            kinds,
            [
                Some(VIDEO_PARAMETER_SET),
                Some(SEQUENCE_PARAMETER_SET),
                Some(PICTURE_PARAMETER_SET)
            ]
        );
    }

    #[test]
    fn a_stream_says_how_far_it_reorders_its_size_profile_and_pixels_shape() {
        let facts = ParameterSets::among(&units_after_start_codes(CLIP))
            .facts()
            .unwrap();
        assert_eq!(facts.reorder_depth, 2);
        assert_eq!(
            facts.size,
            Some(PictureSize {
                width: 320,
                height: 240
            })
        );
        assert_eq!(facts.profile, 1, "the Main profile");
        assert_eq!(facts.level, Some(60), "level 2, thirty times its number");
        assert_eq!(
            facts.pixel_aspect,
            Some(PixelAspect {
                width: 1,
                height: 1
            }),
            "x265 says its pixels are square"
        );
    }

    #[test]
    fn a_main_10_stream_says_its_profile() {
        let facts = ParameterSets::among(&units_after_start_codes(MAIN_10_CLIP))
            .facts()
            .unwrap();
        assert_eq!(facts.profile, 2, "the Main 10 profile");
    }

    /// x265 sends a video signal of video's range with no colour description, which reads as
    /// what the standard infers of a stream that sends none.
    #[test]
    fn a_signal_that_reads_as_the_standards_inference_says_nothing() {
        let facts = ParameterSets::among(&units_after_start_codes(CLIP))
            .facts()
            .unwrap();
        assert_eq!(facts.color.range, None);
        assert_eq!(facts.color.primaries, None);
    }

    #[test]
    fn a_kind_sent_again_takes_the_place_of_the_one_held() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let other_picture_set = ParameterSets {
            picture: vec![vec![PICTURE_PARAMETER_SET << 1, 1, 0xC1]],
            ..ParameterSets::default()
        };
        let replaced = sets.replaced_by(other_picture_set.clone());
        assert_eq!(replaced.video, sets.video);
        assert_eq!(replaced.sequence, sets.sequence);
        assert_eq!(replaced.picture, other_picture_set.picture);
        assert_eq!(sets.replaced_by(ParameterSets::default()), sets);
    }

    #[test]
    fn a_record_holds_the_sets_and_the_size_of_a_length() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let record = record_of(&sets);
        assert_eq!(ParameterSets::of_record(&record), Ok((sets, 4)));
        assert!(ParameterSets::of_record(&[1, 2, 3]).is_err());
    }

    #[test]
    fn a_pixels_shape_is_the_streams_own_or_one_of_the_tables() {
        assert_eq!(
            pixel_aspect(&AspectRatioInfo::Predefined(AspectRatioIdc(14))),
            Some(PixelAspect {
                width: 4,
                height: 3
            })
        );
        assert_eq!(
            pixel_aspect(&AspectRatioInfo::ExtendedSar {
                sar_width: 8,
                sar_height: 9
            }),
            Some(PixelAspect {
                width: 8,
                height: 9
            })
        );
        assert_eq!(
            pixel_aspect(&AspectRatioInfo::Predefined(AspectRatioIdc(0))),
            None
        );
    }
}
