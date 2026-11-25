//! A short H.264 stream and what is read of it, for the decoders' tests: here, and in each
//! host's decoder, which decodes the same clip.

use super::ParameterSets;
use crate::framing::units_after_start_codes;

/// Twelve pictures of 320 by 240 in the High profile, each picture decoded up to two ahead of
/// ones shown before it, as x264 made them (`ffmpeg -f lavfi -i testsrc=size=320x240:rate=30
/// -frames:v 12 -c:v libx264 -bf 2 -x264-params aud=1:keyint=12:bframes=2`, then
/// `-bsf:v h264_mp4toannexb -f h264`).
pub const CLIP: &[u8] = include_bytes!("test_clip.h264");

/// A unit that begins a picture's units in the clip: an access unit delimiter.
const DELIMITER: u8 = 9;

/// The clip's pictures in the order they are decoded, each its units.
pub fn pictures_of_the_clip() -> Vec<Vec<&'static [u8]>> {
    let mut pictures: Vec<Vec<&[u8]>> = Vec::new();
    for unit in units_after_start_codes(CLIP) {
        if unit[0] & 0x1F == DELIMITER {
            pictures.push(Vec::new());
        }
        if let Some(picture) = pictures.last_mut() {
            picture.push(unit);
        }
    }
    pictures
}

/// The configuration record (`avcC`) a file holds for a stream of `sets`, its units after
/// lengths of four bytes.
pub fn record_of(sets: &ParameterSets) -> Vec<u8> {
    let sequence: Vec<&[u8]> = sets.sequence_sets().collect();
    let picture: Vec<&[u8]> = sets.picture_sets().collect();
    let first = sequence[0];
    let mut record = vec![1, first[1], first[2], first[3], 0xFF];
    record.push(0xE0 | sequence.len() as u8);
    for set in &sequence {
        record.extend_from_slice(&(set.len() as u16).to_be_bytes());
        record.extend_from_slice(set);
    }
    record.push(picture.len() as u8);
    for set in &picture {
        record.extend_from_slice(&(set.len() as u16).to_be_bytes());
        record.extend_from_slice(set);
    }
    record
}
