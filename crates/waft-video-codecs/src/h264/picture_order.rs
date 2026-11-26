//! Where each H.264 picture stands in the order its stream is shown in: its picture order count,
//! worked out from its first slice's header and the stream's parameter sets as the standard does
//! (H.264, 8.2.1). A system's decoder that hands pictures back in the order they were decoded
//! (VideoToolbox) leaves their order to its caller, which orders by this count as the other
//! systems' decoders do, so a stream comes out the same everywhere, whatever times an app gives
//! its chunks.
//!
//! A count is told from the picture before it, so the counts stand only among the pictures of
//! one sequence: a key picture, or one that forgets every picture before it, begins the count
//! again.
//!
//! The invariant: every picture decoded is counted, in the order they are decoded. One left out
//! leaves the counts after it wrong until the next sequence begins.

use h264_reader::Context;
use h264_reader::nal::pps::PicParameterSet;
use h264_reader::nal::slice::{
    DecRefPicMarking, Field, FieldPic, MemoryManagementControlOperation, PicOrderCountLsb,
    SliceHeader,
};
use h264_reader::nal::sps::{PicOrderCntType, SeqParameterSet};
use h264_reader::nal::{Nal, RefNal, UnitType};
use h264_reader::rbsp::{BitReader, decode_nal};

use super::ParameterSets;
use crate::presentation_order::Place;

/// A stream's pictures counted as they are decoded.
#[derive(Default)]
pub struct PictureOrder {
    sets: Context,
    previous: Previous,
}

impl PictureOrder {
    /// Counts with `sets` from now on; why not, for a set that cannot be read.
    pub fn adopt(&mut self, sets: &ParameterSets) -> Result<(), String> {
        let mut read = Context::new();
        for unit in sets.sequence_sets() {
            let payload = decode_nal(unit).map_err(|error| format!("a parameter set: {error}"))?;
            let set = SeqParameterSet::from_bits(BitReader::new(&payload[..]))
                .map_err(|error| format!("a sequence parameter set: {error:?}"))?;
            read.put_seq_param_set(set);
        }
        for unit in sets.picture_sets() {
            let payload = decode_nal(unit).map_err(|error| format!("a parameter set: {error}"))?;
            let set = PicParameterSet::from_bits(&read, BitReader::new(&payload[..]))
                .map_err(|error| format!("a picture parameter set: {error:?}"))?;
            read.put_pic_param_set(set);
        }
        self.sets = read;
        Ok(())
    }

    /// The place of the picture `units` code, told from its first slice; none where no unit is
    /// a slice, and why not, for a slice whose header cannot be read.
    pub fn place_of(&mut self, units: &[&[u8]]) -> Result<Option<Place>, String> {
        let Some(slice) = units.iter().find_map(|unit| first_slice(unit)) else {
            return Ok(None);
        };
        let header = slice
            .header()
            .map_err(|error| format!("a slice's first byte: {error:?}"))?;
        let (read, sequence, _) =
            SliceHeader::from_bits(&self.sets, &mut slice.rbsp_bits(), header, None)
                .map_err(|error| format!("a slice's header: {error:?}"))?;
        let slice = Slice {
            key: header.nal_unit_type() == UnitType::SliceLayerWithoutPartitioningIdr,
            reference: header.nal_ref_idc() != 0,
            forgets_all: forgets_all(&read),
            frame_num: i64::from(read.frame_num),
            field: match read.field_pic {
                FieldPic::Frame => None,
                FieldPic::Field(field) => Some(field),
            },
            order: read.pic_order_cnt_lsb,
        };
        let count = self.previous.count_next(
            &sequence.pic_order_cnt,
            1 << sequence.log2_max_frame_num(),
            &slice,
        )?;
        Ok(Some(Place {
            // A picture that forgets all before it stands first among those after it.
            count: if slice.forgets_all { 0 } else { count },
            begins_sequence: slice.key || slice.forgets_all,
        }))
    }
}

/// `unit` as a slice of a picture, if it is one whose header says the picture's order.
fn first_slice(unit: &[u8]) -> Option<RefNal<'_>> {
    let unit_type = UnitType::for_id(*unit.first()? & 0x1F).ok()?;
    matches!(
        unit_type,
        UnitType::SliceLayerWithoutPartitioningIdr | UnitType::SliceLayerWithoutPartitioningNonIdr
    )
    .then(|| RefNal::new(unit, &[], true))
}

/// Whether the slice's picture marks every picture before it as no longer referred to (the
/// memory management control operation 5).
fn forgets_all(slice: &SliceHeader) -> bool {
    matches!(
        &slice.dec_ref_pic_marking,
        Some(DecRefPicMarking::Adaptive(operations)) if operations
            .iter()
            .any(|operation| matches!(
                operation,
                MemoryManagementControlOperation::AllRefPicturesUnused
            ))
    )
}

/// What a slice's header says that its picture's count is told from.
struct Slice {
    /// Whether the picture decodes with nothing before it.
    key: bool,
    /// Whether later pictures may refer to it.
    reference: bool,
    forgets_all: bool,
    frame_num: i64,
    /// Which field it is, for a picture that is one field of a frame.
    field: Option<Field>,
    /// The order fields the header carries, as the sequence's way of counting has them.
    order: Option<PicOrderCountLsb>,
}

/// What the next picture's count is told from: of the reference picture before it where the
/// count is sent in parts, and of the picture before it where it follows the frame numbers.
#[derive(Default)]
struct Previous {
    count_high: i64,
    count_low: i64,
    frame_num: i64,
    frame_num_offset: i64,
}

impl Previous {
    /// The count of the picture `slice` begins, in a sequence counting as `counting` says with
    /// frame numbers below `frame_num_limit`; why not, where the header's order fields are not
    /// the ones that way of counting sends.
    fn count_next(
        &mut self,
        counting: &PicOrderCntType,
        frame_num_limit: i64,
        slice: &Slice,
    ) -> Result<i64, String> {
        if slice.key {
            *self = Previous::default();
        }
        let wrapped = self.frame_num > slice.frame_num;
        let frame_num_offset = self.frame_num_offset + if wrapped { frame_num_limit } else { 0 };
        let frame_count = frame_num_offset + slice.frame_num;
        let count = match (counting, &slice.order) {
            (
                PicOrderCntType::TypeZero {
                    log2_max_pic_order_cnt_lsb_minus4: bits,
                },
                Some(PicOrderCountLsb::Frame(low)),
            ) => self.count_in_parts(slice, 1 << (bits + 4), i64::from(*low), 0),
            (
                PicOrderCntType::TypeZero {
                    log2_max_pic_order_cnt_lsb_minus4: bits,
                },
                Some(PicOrderCountLsb::FieldsAbsolute {
                    pic_order_cnt_lsb: low,
                    delta_pic_order_cnt_bottom: bottom,
                }),
            ) => self.count_in_parts(slice, 1 << (bits + 4), i64::from(*low), i64::from(*bottom)),
            (
                PicOrderCntType::TypeOne {
                    offset_for_non_ref_pic,
                    offset_for_top_to_bottom_field,
                    offsets_for_ref_frame,
                    ..
                },
                Some(PicOrderCountLsb::FieldsDelta([first, second])),
            ) => {
                let expected = expected_count(
                    offsets_for_ref_frame,
                    frame_count,
                    slice.reference,
                    i64::from(*offset_for_non_ref_pic),
                );
                let top = expected.saturating_add(i64::from(*first));
                let bottom = top
                    .saturating_add(i64::from(*offset_for_top_to_bottom_field))
                    .saturating_add(i64::from(*second));
                match slice.field {
                    None => top.min(bottom),
                    Some(Field::Top) => top,
                    Some(Field::Bottom) => bottom.saturating_sub(i64::from(*second)),
                }
            }
            // Two for each frame number, a picture nothing refers to one before its frame's.
            (PicOrderCntType::TypeTwo, None) if slice.key => 0,
            (PicOrderCntType::TypeTwo, None) => 2 * frame_count - i64::from(!slice.reference),
            _ => return Err("a slice's order fields are not its sequence's".to_owned()),
        };
        self.frame_num = if slice.forgets_all {
            0
        } else {
            slice.frame_num
        };
        self.frame_num_offset = if slice.forgets_all {
            0
        } else {
            frame_num_offset
        };
        Ok(count)
    }

    /// The count of a picture whose header sends its low part, below `low_limit`, and for a
    /// frame how far its bottom field's count is from its top's: the high part goes up or down
    /// one step where the low part wrapped since the reference picture before (8.2.1.1).
    fn count_in_parts(&mut self, slice: &Slice, low_limit: i64, low: i64, bottom: i64) -> i64 {
        let half = low_limit / 2;
        let high = if low < self.count_low && self.count_low - low >= half {
            self.count_high + low_limit
        } else if low > self.count_low && low - self.count_low > half {
            self.count_high - low_limit
        } else {
            self.count_high
        };
        let first = high + low;
        let count = match slice.field {
            None => first.min(first + bottom),
            Some(_) => first,
        };
        if slice.reference && slice.forgets_all {
            // Counted again from itself: what is left of its top field's count.
            self.count_high = 0;
            self.count_low = match slice.field {
                None => first - count,
                Some(_) => 0,
            };
        } else if slice.reference {
            self.count_high = high;
            self.count_low = low;
        }
        count
    }
}

/// The count a sequence expects of its picture at `frame_count`, where it sends only how far a
/// picture is from that: `offsets` is how far each reference frame of its cycle is after the
/// one before, and a picture nothing refers to stands `offset_for_unreferred` from the frame
/// before it (8.2.1.2).
fn expected_count(
    offsets: &[i32],
    frame_count: i64,
    reference: bool,
    offset_for_unreferred: i64,
) -> i64 {
    let cycle = offsets.len() as i64;
    let frames = if cycle == 0 { 0 } else { frame_count };
    let frames = if reference { frames } else { frames - 1 };
    let along = |offsets: &[i32]| offsets.iter().map(|offset| i64::from(*offset)).sum::<i64>();
    let expected = if frames > 0 {
        let whole_cycles = (frames - 1) / cycle;
        let into_cycle = ((frames - 1) % cycle) as usize;
        whole_cycles
            .saturating_mul(along(offsets))
            .saturating_add(along(&offsets[..=into_cycle]))
    } else {
        0
    };
    if reference {
        expected
    } else {
        expected.saturating_add(offset_for_unreferred)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_clip::{CLIP, pictures_of_the_clip};
    use super::*;
    use crate::framing::units_after_start_codes;

    /// When each of the clip's pictures is shown, in the order they are decoded, counted in
    /// pictures: what `ffprobe` reads of the clip's packets.
    const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

    fn slice(frame_num: i64, reference: bool, order: Option<PicOrderCountLsb>) -> Slice {
        Slice {
            key: false,
            reference,
            forgets_all: false,
            frame_num,
            field: None,
            order,
        }
    }

    #[test]
    fn a_streams_pictures_are_counted_in_the_order_they_are_shown() {
        let mut order = PictureOrder::default();
        order
            .adopt(&ParameterSets::among(&units_after_start_codes(CLIP)))
            .unwrap();
        let places: Vec<Place> = pictures_of_the_clip()
            .iter()
            .map(|units| order.place_of(units).unwrap().expect("a picture"))
            .collect();
        // x264 counts two for each picture.
        let shown_at: Vec<i64> = places.iter().map(|place| place.count / 2).collect();
        assert_eq!(shown_at, SHOWN_AT);
        let begins: Vec<bool> = places.iter().map(|place| place.begins_sequence).collect();
        assert!(begins[0]);
        assert!(begins[1..].iter().all(|begins| !begins));
    }

    #[test]
    fn units_with_no_slice_among_them_are_no_picture() {
        let mut order = PictureOrder::default();
        let delimiter: &[u8] = &[9, 0xF0];
        assert_eq!(order.place_of(&[delimiter]), Ok(None));
    }

    #[test]
    fn a_slice_of_sets_that_have_not_come_cannot_be_placed() {
        let mut order = PictureOrder::default();
        let pictures = pictures_of_the_clip();
        assert!(order.place_of(&pictures[1]).is_err());
    }

    /// A count sent in four bits wraps at sixteen: after 14, a 2 is eighteen; a picture shown
    /// before that one, sent as 12 after it, is twelve and not twenty-eight; and nothing is
    /// counted from it, since nothing refers to it.
    #[test]
    fn a_count_sent_in_parts_goes_on_where_its_low_part_wraps() {
        let counting = PicOrderCntType::TypeZero {
            log2_max_pic_order_cnt_lsb_minus4: 0,
        };
        let sent = |low| Some(PicOrderCountLsb::Frame(low));
        let mut previous = Previous::default();
        let mut count = |frame_num, reference, low| {
            previous
                .count_next(&counting, 16, &slice(frame_num, reference, sent(low)))
                .unwrap()
        };
        assert_eq!(count(1, true, 6), 6);
        assert_eq!(count(2, true, 14), 14);
        assert_eq!(count(3, true, 2), 18);
        assert_eq!(count(4, false, 12), 12);
        assert_eq!(count(4, true, 6), 22);
    }

    /// Counting by frame numbers alone, which wrap at sixteen here: two for each frame, and a
    /// picture nothing refers to one before its frame's.
    #[test]
    fn a_count_follows_the_frame_numbers_where_none_is_sent() {
        let mut previous = Previous::default();
        let mut count = |frame_num, reference| {
            previous
                .count_next(
                    &PicOrderCntType::TypeTwo,
                    16,
                    &slice(frame_num, reference, None),
                )
                .unwrap()
        };
        assert_eq!(count(1, true), 2);
        assert_eq!(count(2, false), 3);
        assert_eq!(count(15, true), 30);
        assert_eq!(count(0, true), 32);
    }

    /// A cycle of two reference frames, 2 and then 4 after the one before: the fourth frame is
    /// one whole cycle and one frame in, and a picture nothing refers to stands one before the
    /// frame before it.
    #[test]
    fn a_count_is_expected_from_the_sequences_cycle_where_only_its_distance_is_sent() {
        assert_eq!(expected_count(&[2, 4], 1, true, -1), 2);
        assert_eq!(expected_count(&[2, 4], 2, true, -1), 6);
        assert_eq!(expected_count(&[2, 4], 3, true, -1), 8);
        assert_eq!(expected_count(&[2, 4], 3, false, -1), 5);
        assert_eq!(expected_count(&[], 3, true, -1), 0);
    }

    #[test]
    fn a_key_picture_begins_the_count_again() {
        let mut previous = Previous::default();
        let counting = PicOrderCntType::TypeTwo;
        previous
            .count_next(&counting, 16, &slice(5, true, None))
            .unwrap();
        let key = Slice {
            key: true,
            ..slice(0, true, None)
        };
        assert_eq!(previous.count_next(&counting, 16, &key), Ok(0));
        assert_eq!(
            previous.count_next(&counting, 16, &slice(1, true, None)),
            Ok(2)
        );
    }

    #[test]
    fn order_fields_that_are_not_the_sequences_cannot_be_counted() {
        let mut previous = Previous::default();
        let sent = Some(PicOrderCountLsb::Frame(0));
        assert!(
            previous
                .count_next(&PicOrderCntType::TypeTwo, 16, &slice(1, true, sent))
                .is_err()
        );
    }
}
