//! Pictures in the order they are shown. VideoToolbox hands pictures back in the order their
//! chunks were decoded, which a stream with pictures that refer forward is not shown in, so each
//! waits here until none that is shown before it can still come.
//!
//! A stream says how many pictures may be decoded ahead of one shown before them (its reordering
//! depth). A picture is let out once more than that many are held, the earliest first. A key
//! frame that begins a new sequence is shown after everything decoded before it, so those are let
//! out as it arrives.
//!
//! The invariant: pictures leave in ascending order of their time, as long as the stream keeps to
//! the depth it said.

use std::collections::BTreeMap;

pub(super) struct PresentationOrder<Picture> {
    /// How many pictures may be decoded ahead of one shown before them.
    depth: usize,
    held: BTreeMap<i64, Picture>,
}

impl<Picture> PresentationOrder<Picture> {
    pub(super) fn new(depth: usize) -> Self {
        PresentationOrder {
            depth,
            held: BTreeMap::new(),
        }
    }

    /// The stream's depth from now on, as other parameter sets say it.
    pub(super) fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Takes `picture`, shown at `time`, and answers those that can be shown now, in order.
    /// `begins_sequence` for a key frame nothing decoded later is shown before.
    pub(super) fn take(
        &mut self,
        time: i64,
        picture: Picture,
        begins_sequence: bool,
    ) -> Vec<Picture> {
        let mut shown = if begins_sequence {
            self.all()
        } else {
            Vec::new()
        };
        self.held.insert(time, picture);
        while self.held.len() > self.depth {
            let (_, earliest) = self.held.pop_first().expect("more than none are held");
            shown.push(earliest);
        }
        shown
    }

    /// Every picture still held, in order: what a drain shows.
    pub(super) fn all(&mut self) -> Vec<Picture> {
        std::mem::take(&mut self.held).into_values().collect()
    }

    /// Drops every picture held, as a reset does.
    pub(super) fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Takes each of `times` as a picture named by its time.
    fn shown_as_taken(order: &mut PresentationOrder<i64>, times: &[i64]) -> Vec<Vec<i64>> {
        times
            .iter()
            .map(|time| order.take(*time, *time, false))
            .collect()
    }

    #[test]
    fn a_stream_that_does_not_reorder_shows_each_picture_as_it_comes() {
        let mut order = PresentationOrder::new(0);
        assert_eq!(
            shown_as_taken(&mut order, &[0, 1, 2]),
            [vec![0], vec![1], vec![2]]
        );
    }

    /// Decoded I P B B, shown I B B P: the pictures at 1 and 2 refer forward to the one at 3.
    #[test]
    fn pictures_decoded_ahead_wait_for_those_shown_before_them() {
        let mut order = PresentationOrder::new(1);
        assert_eq!(
            shown_as_taken(&mut order, &[0, 3, 1, 2, 6, 4, 5]),
            [vec![], vec![0], vec![1], vec![2], vec![3], vec![4], vec![5]]
        );
        assert_eq!(order.all(), [6]);
    }

    #[test]
    fn a_key_frame_that_begins_a_sequence_follows_all_decoded_before_it() {
        let mut order = PresentationOrder::new(2);
        assert_eq!(order.take(0, 0, true), Vec::<i64>::new());
        assert_eq!(order.take(2, 2, false), Vec::<i64>::new());
        assert_eq!(order.take(10, 10, true), [0, 2]);
        assert_eq!(order.all(), [10]);
    }

    #[test]
    fn a_reset_drops_what_is_held() {
        let mut order = PresentationOrder::new(2);
        order.take(0, 0, true);
        order.clear();
        assert_eq!(order.all(), Vec::<i64>::new());
    }
}
