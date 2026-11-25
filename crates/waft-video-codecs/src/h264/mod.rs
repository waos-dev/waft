//! H.264: a stream's setup and its units, its parameter sets (`parameter_sets`), and where each
//! picture stands in the order its stream is shown in (`picture_order`).
//!
//! The invariant: a stream's parameter sets are those it gave last of each kind, wherever they
//! came from, its setup or its chunks; a decoder configured with any others decodes a different
//! stream.

mod parameter_sets;
mod picture_order;
#[cfg(any(test, feature = "test-support"))]
pub mod test_clip;

use waft_embedder::video_decoders::NalFormat;

use crate::framing::{Framing, units_after_start_codes};

pub use parameter_sets::{ParameterSets, is_parameter_set, unit_type};
pub use picture_order::PictureOrder;

/// A stream's setup as a decoder takes it: how its chunks frame their units, and the parameter
/// sets given ahead, which may be none; why not, for a configuration record that is no such
/// record.
pub fn setup(format: NalFormat) -> Result<(Framing, ParameterSets), String> {
    match format {
        NalFormat::LengthPrefixed(record) => {
            let (sets, length_size) = ParameterSets::of_record(&record)?;
            Ok((Framing::Lengths(length_size), sets))
        }
        NalFormat::StartCodes(ahead) => {
            let ahead = ahead.unwrap_or_default();
            let sets = ParameterSets::among(&units_after_start_codes(&ahead));
            Ok((Framing::StartCodes, sets))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_clip::{CLIP, record_of};
    use super::*;

    #[test]
    fn a_setup_of_start_codes_takes_the_sets_given_ahead() {
        let (framing, sets) = setup(NalFormat::StartCodes(Some(CLIP.to_vec()))).unwrap();
        assert_eq!(framing, Framing::StartCodes);
        assert!(sets.are_complete());
        let (_, none) = setup(NalFormat::StartCodes(None)).unwrap();
        assert!(!none.are_complete());
    }

    #[test]
    fn a_setup_of_a_record_says_the_size_of_a_length() {
        let sets = ParameterSets::among(&units_after_start_codes(CLIP));
        let (framing, from_record) = setup(NalFormat::LengthPrefixed(record_of(&sets))).unwrap();
        assert_eq!(framing, Framing::Lengths(4));
        assert_eq!(from_record, sets);
        assert!(setup(NalFormat::LengthPrefixed(vec![1, 2, 3])).is_err());
    }
}
