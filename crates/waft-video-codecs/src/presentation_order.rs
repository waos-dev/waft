//! Pictures in the order they are shown, for a system whose decoder hands them back in the order
//! their chunks were decoded (VideoToolbox does): a stream with pictures that refer forward is
//! not shown in that order, so each waits here, by the place its stream gives it (each codec's
//! picture order), until none that is shown before it can still come.
//!
//! A stream says how many pictures may be decoded ahead of one shown before them (its reordering
//! depth). A picture is let out once more than that many are held, the earliest first. A key
//! frame that begins a new sequence is shown after everything decoded before it, so those are let
//! out as it arrives.
//!
//! The invariant: pictures leave in ascending order of their count, as long as the stream keeps
//! to the depth it said, and every picture taken leaves: two with one count leave in the order
//! they came, neither in the other's place.

use std::collections::BTreeMap;

/// Where a picture stands among those of its stream, as its codec tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    /// Its count in the order pictures are shown: a picture with a smaller one is shown before
    /// it.
    pub count: i64,
    /// Whether every picture decoded before it is shown before it, and the count begins again.
    pub begins_sequence: bool,
}

pub struct PresentationOrder<Picture> {
    /// How many pictures may be decoded ahead of one shown before them.
    depth: usize,
    /// Each picture held, by its count and then by when it came.
    held: BTreeMap<(i64, u64), Picture>,
    /// How many pictures have come.
    taken: u64,
}

impl<Picture> PresentationOrder<Picture> {
    pub fn new(depth: usize) -> Self {
        PresentationOrder {
            depth,
            held: BTreeMap::new(),
            taken: 0,
        }
    }

    /// The stream's depth from now on, as other parameter sets say it.
    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    /// Takes `picture`, which stands at `place` in its stream, and answers those that can be
    /// shown now, in order.
    pub fn take(&mut self, place: Place, picture: Picture) -> Vec<Picture> {
        let mut shown = if place.begins_sequence {
            self.all()
        } else {
            Vec::new()
        };
        self.held.insert((place.count, self.taken), picture);
        self.taken += 1;
        while self.held.len() > self.depth {
            let (_, earliest) = self.held.pop_first().expect("more than none are held");
            shown.push(earliest);
        }
        shown
    }

    /// Every picture still held, in order: what a drain shows.
    pub fn all(&mut self) -> Vec<Picture> {
        std::mem::take(&mut self.held).into_values().collect()
    }

    /// Drops every picture held, as a reset does.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within_sequence(count: i64) -> Place {
        Place {
            count,
            begins_sequence: false,
        }
    }

    fn beginning_sequence(count: i64) -> Place {
        Place {
            count,
            begins_sequence: true,
        }
    }

    /// Takes each of `counts` as a picture named by its count.
    fn shown_as_taken(order: &mut PresentationOrder<i64>, counts: &[i64]) -> Vec<Vec<i64>> {
        counts
            .iter()
            .map(|count| order.take(within_sequence(*count), *count))
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
        assert_eq!(
            order.take(beginning_sequence(0), "first"),
            Vec::<&str>::new()
        );
        assert_eq!(order.take(within_sequence(4), "second"), Vec::<&str>::new());
        // The count begins again with it, so it is smaller than those shown before it.
        assert_eq!(
            order.take(beginning_sequence(0), "third"),
            ["first", "second"]
        );
        assert_eq!(order.all(), ["third"]);
    }

    /// Two pictures of a stream that breaks its own rules and counts both the same.
    #[test]
    fn pictures_with_one_count_all_leave_in_the_order_they_came() {
        let mut order = PresentationOrder::new(1);
        let shown: Vec<Vec<&str>> = ["first", "second", "third"]
            .into_iter()
            .map(|picture| order.take(within_sequence(0), picture))
            .collect();
        assert_eq!(shown, [vec![], vec!["first"], vec!["second"]]);
        assert_eq!(order.all(), ["third"]);
    }

    #[test]
    fn a_reset_drops_what_is_held() {
        let mut order = PresentationOrder::new(2);
        order.take(beginning_sequence(0), 0);
        order.clear();
        assert_eq!(order.all(), Vec::<i64>::new());
    }
}
