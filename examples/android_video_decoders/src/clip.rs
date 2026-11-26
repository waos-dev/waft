//! The clip the checks decode, as an app that read it from a file would hand it over: one chunk a
//! picture, in the order they are decoded, framed after start codes or after lengths.

use waft_embedder::video_decoders::EncodedChunk;

const CLIP: &[u8] = include_bytes!("../media/test_clip.h264");

/// When each of the clip's pictures is shown, in the order they are decoded, counted in pictures:
/// what `ffprobe` reads of the clip's packets.
pub(crate) const SHOWN_AT: [i64; 12] = [0, 3, 1, 2, 5, 4, 8, 6, 7, 11, 9, 10];

/// How long a picture shows, in nanoseconds, at thirty a second.
pub(crate) const PICTURE: i64 = 33_333_333;

const ACCESS_UNIT_DELIMITER: u8 = 9;
const SEQUENCE_PARAMETER_SET: u8 = 7;
const PICTURE_PARAMETER_SET: u8 = 8;

/// The time of a picture shown `number` pictures in.
pub(crate) fn time_of(number: i64) -> i64 {
    number * PICTURE
}

/// The clip's chunks in the order they are decoded, each its units after start codes, the first
/// with the stream's parameter sets.
pub(crate) fn chunks_after_start_codes() -> Vec<EncodedChunk> {
    chunks(|units| framed(units, |_| START_CODE.to_vec()))
}

/// The clip's chunks with each unit after four bytes of its length, the parameter sets left to the
/// configuration record (`record`), as MP4 holds them.
pub(crate) fn chunks_after_lengths() -> Vec<EncodedChunk> {
    chunks(|units| {
        let coded: Vec<&[u8]> = units
            .iter()
            .copied()
            .filter(|unit| !is_parameter_set(unit))
            .collect();
        framed(&coded, |unit| (unit.len() as u32).to_be_bytes().to_vec())
    })
}

/// The clip's configuration record (`avcC`), its units' lengths four bytes long.
pub(crate) fn record() -> Vec<u8> {
    let units = units();
    let of_type = |wanted: u8| -> Vec<&[u8]> {
        units
            .iter()
            .copied()
            .filter(|unit| unit_type(unit) == wanted)
            .collect()
    };
    let (sequence, picture) = (
        of_type(SEQUENCE_PARAMETER_SET),
        of_type(PICTURE_PARAMETER_SET),
    );
    let first = sequence[0];
    let mut record = vec![
        1,
        first[1],
        first[2],
        first[3],
        0xFF,
        0xE0 | sequence.len() as u8,
    ];
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

const START_CODE: [u8; 4] = [0, 0, 0, 1];

fn chunks(framed: impl Fn(&[&[u8]]) -> Vec<u8>) -> Vec<EncodedChunk> {
    pictures()
        .iter()
        .zip(SHOWN_AT)
        .enumerate()
        .map(|(decoded, (units, shown))| EncodedChunk {
            data: framed(units),
            timestamp: time_of(shown),
            duration: Some(PICTURE as u64),
            key_frame: decoded == 0,
        })
        .collect()
}

/// `units`, each after what `before` makes of it.
fn framed(units: &[&[u8]], before: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    units
        .iter()
        .flat_map(|unit| [before(unit), unit.to_vec()].concat())
        .collect()
}

/// The clip's pictures, each its units from one delimiter to the next; what comes before the
/// first, x264's note of its settings, is left out, as Apple's check leaves it.
fn pictures() -> Vec<Vec<&'static [u8]>> {
    let mut pictures: Vec<Vec<&[u8]>> = Vec::new();
    for unit in units() {
        if unit_type(unit) == ACCESS_UNIT_DELIMITER {
            pictures.push(Vec::new());
        }
        if let Some(picture) = pictures.last_mut() {
            picture.push(unit);
        }
    }
    pictures
}

/// The clip's units, each without its start code.
fn units() -> Vec<&'static [u8]> {
    let mut starts = Vec::new();
    let mut at = 0;
    while at + 3 <= CLIP.len() {
        if CLIP[at..at + 3] == [0, 0, 1] {
            starts.push(at + 3);
            at += 3;
        } else {
            at += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, &start)| {
            let end = starts.get(index + 1).map_or(CLIP.len(), |next| next - 3);
            let unit = &CLIP[start..end];
            let last = unit
                .iter()
                .rposition(|byte| *byte != 0)
                .map_or(0, |at| at + 1);
            &unit[..last]
        })
        .filter(|unit| !unit.is_empty())
        .collect()
}

fn unit_type(unit: &[u8]) -> u8 {
    unit[0] & 0x1F
}

fn is_parameter_set(unit: &[u8]) -> bool {
    matches!(
        unit_type(unit),
        SEQUENCE_PARAMETER_SET | PICTURE_PARAMETER_SET
    )
}
