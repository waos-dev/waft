//! Short HEVC streams and what is read of them, for the decoders' tests: here, and in each host's
//! decoder, which decodes the same clips.

use std::io::Cursor;

use scuffle_h265::{
    ConstantFrameRate, HEVCDecoderConfigurationRecord, NALUnitType, NaluArray, NumTemporalLayers,
    ParallelismType, SpsNALUnit,
};

use super::{ParameterSets, unit_type};
use crate::framing::units_after_start_codes;

/// Twelve pictures of 320 by 240 in the Main profile, each picture decoded up to two ahead of
/// ones shown before it, as x265 made them (`ffmpeg -f lavfi -i testsrc=size=320x240:rate=30
/// -frames:v 12 -pix_fmt yuv420p -c:v libx265 -x265-params keyint=12:bframes=2:aud=1:info=0
/// -f hevc`).
pub const CLIP: &[u8] = include_bytes!("test_clip.h265");

/// The same twelve pictures in the Main 10 profile, ten bits to a sample, as x265 made them
/// (the same command with `-pix_fmt yuv420p10le`).
pub const MAIN_10_CLIP: &[u8] = include_bytes!("test_clip_main10.h265");

/// A unit that begins a picture's units in the clips: an access unit delimiter.
const DELIMITER: u8 = 35;

/// `clip`'s pictures in the order they are decoded, each its units.
pub fn pictures_of(clip: &[u8]) -> Vec<Vec<&[u8]>> {
    let mut pictures: Vec<Vec<&[u8]>> = Vec::new();
    for unit in units_after_start_codes(clip) {
        if unit_type(unit) == Some(DELIMITER) {
            pictures.push(Vec::new());
        }
        if let Some(picture) = pictures.last_mut() {
            picture.push(unit);
        }
    }
    pictures
}

/// The configuration record (`hvcC`) a file holds for a stream of `sets`, its units after
/// lengths of four bytes, its profile as the first sequence set says.
pub fn record_of(sets: &ParameterSets) -> Vec<u8> {
    let first = sets
        .sequence_sets()
        .next()
        .expect("a sequence parameter set");
    let sequence = SpsNALUnit::parse(Cursor::new(first))
        .expect("a sequence parameter set that reads")
        .rbsp;
    let profile = &sequence.profile_tier_level.general_profile;
    let array = |nal_unit_type, units: Vec<&[u8]>| NaluArray {
        array_completeness: true,
        nal_unit_type,
        nalus: units.into_iter().map(|unit| unit.to_vec().into()).collect(),
    };
    let record = HEVCDecoderConfigurationRecord {
        general_profile_space: profile.profile_space,
        general_tier_flag: profile.tier_flag,
        general_profile_idc: profile.profile_idc,
        general_profile_compatibility_flags: profile.profile_compatibility_flag,
        general_constraint_indicator_flags: 0,
        general_level_idc: profile.level_idc.unwrap_or_default(),
        min_spatial_segmentation_idc: 0,
        parallelism_type: ParallelismType::MixedOrUnknown,
        chroma_format_idc: sequence.chroma_format_idc,
        bit_depth_luma_minus8: sequence.bit_depth_luma_minus8,
        bit_depth_chroma_minus8: sequence.bit_depth_chroma_minus8,
        avg_frame_rate: 0,
        constant_frame_rate: ConstantFrameRate::Unknown,
        num_temporal_layers: NumTemporalLayers::NotScalable,
        temporal_id_nested: true,
        length_size_minus_one: 3,
        arrays: vec![
            array(NALUnitType::VpsNut, sets.video_sets().collect()),
            array(NALUnitType::SpsNut, sets.sequence_sets().collect()),
            array(NALUnitType::PpsNut, sets.picture_sets().collect()),
        ],
    };
    let mut bytes = Vec::new();
    record.mux(&mut bytes).expect("a record written to memory");
    bytes
}
