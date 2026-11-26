//! Where each HEVC picture stands in the order its stream is shown in: its picture order count,
//! worked out from its first slice segment's header and the stream's parameter sets as the
//! standard does (H.265, 8.3.1). A system's decoder that hands pictures back in the order they
//! were decoded (VideoToolbox) leaves their order to its caller, which orders by this count as
//! the other systems' decoders do.
//!
//! A slice header sends the count's low bits; its high part is told from the picture before it
//! that is of the lowest sub-layer and that later pictures may refer to. An intra random access
//! point that begins a coded video sequence (an IDR or BLA picture, or a CRA picture that comes
//! first or after an end of sequence) begins the count again.
//!
//! The invariant: every picture decoded is counted, in the order they are decoded, and with the
//! parameter sets its stream gave last. One left out leaves the counts after it wrong until the
//! next sequence begins.

use std::collections::HashMap;
use std::io;

use scuffle_bytes_util::{BitReader, EmulationPreventionIo};
use scuffle_expgolomb::BitReaderExpGolombExt;
use scuffle_h265::SpsNALUnit;

use super::{KEY_SLICES, ParameterSets, unit_type};
use crate::presentation_order::Place;

/// The unit types of slices of an IDR picture, whose header sends no count.
const IDR_SLICES: [u8; 2] = [19, 20];
/// The unit types of slices of a BLA picture, which begins the count again.
const BLA_SLICES: [u8; 3] = [16, 17, 18];
/// The unit types of slices of a RADL or RASL picture, which lead the key picture they follow.
const LEADING_SLICES: std::ops::RangeInclusive<u8> = 6..=9;
/// The unit types of slices, every other one in the standard's table, reserved or not.
const SLICES: std::ops::RangeInclusive<u8> = 0..=31;
/// The unit type of the end of a coded video sequence.
const END_OF_SEQUENCE: u8 = 36;

/// A stream's pictures counted as they are decoded.
#[derive(Default)]
pub struct PictureOrder {
    sequences: HashMap<u64, Sequence>,
    pictures: HashMap<u64, Picture>,
    previous: Previous,
}

/// What a sequence parameter set says that a slice header is read with.
struct Sequence {
    /// How many low bits of the count a slice header sends.
    count_low_bits: u8,
    /// Whether each colour plane is coded as a picture of its own, its slices saying which.
    separate_colour_planes: bool,
}

/// What a picture parameter set says that a slice header is read with.
struct Picture {
    sequence: u64,
    /// Whether a slice header says whether its picture is shown.
    output_flag: bool,
    /// How many bits a slice header holds for later versions of the standard.
    extra_slice_header_bits: u8,
}

/// What the next picture's count is told from.
struct Previous {
    /// The low and high parts of the count of the last picture of the lowest sub-layer that
    /// later pictures may refer to.
    count_low: i64,
    count_high: i64,
    /// Whether the next picture begins a coded video sequence where it is a CRA picture: the
    /// first, or the first after an end of sequence.
    sequence_ended: bool,
}

impl Default for Previous {
    fn default() -> Self {
        Previous {
            count_low: 0,
            count_high: 0,
            sequence_ended: true,
        }
    }
}

impl PictureOrder {
    /// Counts with `sets` from now on; why not, for a set that cannot be read.
    pub fn adopt(&mut self, sets: &ParameterSets) -> Result<(), String> {
        let mut sequences = HashMap::new();
        for unit in sets.sequence_sets() {
            let set = SpsNALUnit::parse(unit)
                .map_err(|error| format!("a sequence parameter set: {error}"))?
                .rbsp;
            sequences.insert(
                set.sps_seq_parameter_set_id,
                Sequence {
                    count_low_bits: set.log2_max_pic_order_cnt_lsb_minus4 + 4,
                    separate_colour_planes: set.separate_colour_plane_flag,
                },
            );
        }
        let mut pictures = HashMap::new();
        for unit in sets.picture_sets() {
            let (id, picture) = read_picture_set(unit)
                .map_err(|error| format!("a picture parameter set: {error}"))?;
            pictures.insert(id, picture);
        }
        self.sequences = sequences;
        self.pictures = pictures;
        Ok(())
    }

    /// The place of the picture `units` code, told from its first slice segment; none where no
    /// unit is a picture's first, and why not, for a header that cannot be read or names sets
    /// that have not come. An end of sequence among the units makes the next CRA picture begin
    /// one.
    pub fn place_of(&mut self, units: &[&[u8]]) -> Result<Option<Place>, String> {
        let first = units
            .iter()
            .find(|unit| is_first_slice_segment(unit))
            .copied();
        let place = match first {
            Some(slice) => Some(self.count(slice)?),
            None => None,
        };
        if units
            .iter()
            .any(|unit| unit_type(unit) == Some(END_OF_SEQUENCE))
        {
            self.previous.sequence_ended = true;
        }
        Ok(place)
    }

    /// The place of the picture whose first slice segment is `slice`.
    fn count(&mut self, slice: &[u8]) -> Result<Place, String> {
        let kind = unit_type(slice).expect("a slice has a header");
        let temporal_id = (slice[1] & 0x07).saturating_sub(1);
        let key = KEY_SLICES.contains(&kind);
        let begins_sequence = IDR_SLICES.contains(&kind)
            || BLA_SLICES.contains(&kind)
            || (key && self.previous.sequence_ended);
        let low = self
            .count_low_of(slice, kind)
            .map_err(|error| format!("a slice segment's header: {error}"))?;
        let (low, high) = match low {
            None => (0, 0),
            Some((low, _)) if begins_sequence => (low, 0),
            Some((low, low_limit)) => (low, self.previous.count_high_for(low, low_limit)),
        };
        // Neither a leading picture nor one of a higher sub-layer, nor one that no picture of its
        // sub-layer refers to, is counted from.
        let referred_to_at_its_layer = !(kind <= 14 && kind % 2 == 0);
        if temporal_id == 0 && !LEADING_SLICES.contains(&kind) && referred_to_at_its_layer {
            self.previous.count_low = low;
            self.previous.count_high = high;
        }
        if key {
            self.previous.sequence_ended = false;
        }
        Ok(Place {
            count: high + low,
            begins_sequence,
        })
    }

    /// The low part of the count `slice`, a first slice segment of unit type `kind`, sends, and
    /// the limit it stays below; none for an IDR picture's, which sends none.
    fn count_low_of(&self, slice: &[u8], kind: u8) -> io::Result<Option<(i64, i64)>> {
        let mut bits = BitReader::new(EmulationPreventionIo::new(&slice[2..]));
        bits.read_bit()?; // first_slice_segment_in_pic_flag, set
        if KEY_SLICES.contains(&kind) {
            bits.read_bit()?; // no_output_of_prior_pics_flag
        }
        let picture_id = bits.read_exp_golomb()?;
        let picture = self.pictures.get(&picture_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "its picture set has not come")
        })?;
        let sequence = self.sequences.get(&picture.sequence).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "its sequence set has not come")
        })?;
        if IDR_SLICES.contains(&kind) {
            return Ok(None);
        }
        // A picture's first segment is never a dependent one, and sends no address.
        for _ in 0..picture.extra_slice_header_bits {
            bits.read_bit()?; // slice_reserved_flag
        }
        bits.read_exp_golomb()?; // slice_type
        if picture.output_flag {
            bits.read_bit()?; // pic_output_flag
        }
        if sequence.separate_colour_planes {
            bits.read_bits(2)?; // colour_plane_id
        }
        let low = bits.read_bits(sequence.count_low_bits)? as i64;
        Ok(Some((low, 1 << sequence.count_low_bits)))
    }
}

impl Previous {
    /// The high part of the count whose low part is `low`, below `low_limit`: the one before,
    /// a step up or down where the low part wrapped since (8.3.1).
    fn count_high_for(&self, low: i64, low_limit: i64) -> i64 {
        let half = low_limit / 2;
        if low < self.count_low && self.count_low - low >= half {
            self.count_high + low_limit
        } else if low > self.count_low && low - self.count_low > half {
            self.count_high - low_limit
        } else {
            self.count_high
        }
    }
}

/// Whether `unit` is the first slice segment of a picture.
fn is_first_slice_segment(unit: &[u8]) -> bool {
    unit_type(unit).is_some_and(|kind| SLICES.contains(&kind))
        && unit.get(2).is_some_and(|byte| byte & 0x80 != 0)
}

/// The id of the picture parameter set `unit`, and what it says that a slice header is read
/// with.
fn read_picture_set(unit: &[u8]) -> io::Result<(u64, Picture)> {
    let payload = unit
        .get(2..)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no header"))?;
    let mut bits = BitReader::new(EmulationPreventionIo::new(payload));
    let id = bits.read_exp_golomb()?;
    let sequence = bits.read_exp_golomb()?;
    bits.read_bit()?; // dependent_slice_segments_enabled_flag
    let output_flag = bits.read_bit()?;
    let extra_slice_header_bits = bits.read_bits(3)? as u8;
    Ok((
        id,
        Picture {
            sequence,
            output_flag,
            extra_slice_header_bits,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::{CLIP, MAIN_10_CLIP, pictures_of};
    use super::*;
    use crate::framing::units_after_start_codes;

    /// When each of the clips' pictures is shown, in the order they are decoded, counted in
    /// pictures: the counts `ffmpeg -bsf:v trace_headers` reads of their slices.
    const SHOWN_AT: [i64; 12] = [0, 3, 2, 1, 5, 4, 8, 7, 6, 11, 10, 9];
    const MAIN_10_SHOWN_AT: [i64; 12] = [0, 2, 1, 4, 3, 6, 5, 8, 7, 11, 10, 9];

    fn places_of(clip: &[u8]) -> Vec<Place> {
        let mut order = PictureOrder::default();
        order
            .adopt(&ParameterSets::among(&units_after_start_codes(clip)))
            .unwrap();
        pictures_of(clip)
            .iter()
            .map(|units| order.place_of(units).unwrap().expect("a picture"))
            .collect()
    }

    #[test]
    fn a_streams_pictures_are_counted_in_the_order_they_are_shown() {
        for (clip, shown_at) in [(CLIP, SHOWN_AT), (MAIN_10_CLIP, MAIN_10_SHOWN_AT)] {
            let places = places_of(clip);
            let counts: Vec<i64> = places.iter().map(|place| place.count).collect();
            assert_eq!(counts, shown_at);
            assert!(places[0].begins_sequence);
            assert!(places[1..].iter().all(|place| !place.begins_sequence));
        }
    }

    #[test]
    fn units_with_no_slice_among_them_are_no_picture() {
        let mut order = PictureOrder::default();
        let delimiter: &[u8] = &[35 << 1, 1, 0x50];
        assert_eq!(order.place_of(&[delimiter]), Ok(None));
    }

    #[test]
    fn a_slice_of_sets_that_have_not_come_cannot_be_placed() {
        let mut order = PictureOrder::default();
        let pictures = pictures_of(CLIP);
        assert!(order.place_of(&pictures[1]).is_err());
    }

    /// A count sent in four bits wraps at sixteen: after 14, a 2 is eighteen, and a picture
    /// sent as 12 after that is twelve and not twenty-eight.
    #[test]
    fn a_count_goes_on_where_its_low_part_wraps() {
        let mut previous = Previous {
            count_low: 14,
            count_high: 0,
            sequence_ended: false,
        };
        assert_eq!(previous.count_high_for(2, 16), 16);
        previous.count_low = 2;
        previous.count_high = 16;
        assert_eq!(previous.count_high_for(12, 16), 0);
        assert_eq!(previous.count_high_for(6, 16), 16);
    }
}
