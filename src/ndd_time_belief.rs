use std::ops::Bound;

use crate::interval::{Interval, IntervalList};


type RealNumRep = f64;


pub fn get_c_beliefs(a: &[RealNumRep], s: &[RealNumRep]) -> IntervalList<RealNumRep> {
    let mut ret = IntervalList::new_interval(Interval::new(Bound::Unbounded, Bound::Unbounded));
    let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
        Bound::Included(s[3] + -s[2] * 1),
    )]);
    ret = ret.intersection(&tmp);
    let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
        Bound::Included(s[2] + -s[1] * 1),
    )]);
    ret = ret.intersection(&tmp);
    let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
        Bound::Included(s[1] + -s[0] * 1),
    )]);
    ret = ret.intersection(&tmp);
    if !(s[3] <= a[2]) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[3] * 1 / 2 + -s[1] * 1 / 2),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[2]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(-s[2] * 1 + s[3]),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (a[1] <= s[2]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 4 + s[3] * 9 + -s[2] * 5),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[3]) || (s[4] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[4] + -s[3] * 1),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[3]) || (s[4] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[4] + -s[3] * 1),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (a[0] <= s[1]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 18 / 7 + s[3] * 61 / 14 + -s[1] * 25 / 14),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[1]) || (s[2] <= a[1]) || (a[3] <= s[3])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[2] * 61 / 29 + -s[3] * 16 / 29 + -s[1] * 45 / 29),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (a[0] <= s[1]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 8 / 37 + s[3] * 61 / 74 + -s[1] * 45 / 74),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((s[2] <= a[1]) || (a[3] <= s[3])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[2] * 61 / 74 + -s[3] * 8 / 37 + -s[0] * 45 / 74),
        )]);
        ret = ret.intersection(&tmp);
    }
    let tmp = IntervalList::from_interval_lists(vec![
        IntervalList::interval_lower(Bound::Excluded(
            -a[3] * 8 / 37 + s[2] * 61 / 74 + -s[0] * 45 / 74,
        )),
        IntervalList::interval_upper(Bound::Included(
            -s[0] * 125 / 186 + -a[4] * 32 / 93 + s[2] * 125 / 186 + a[3] * 32 / 93,
        )),
        IntervalList::interval_lower(Bound::Included(
            -s[0] * 125 / 186 + -s[4] * 32 / 93 + s[2] * 125 / 186 + a[3] * 32 / 93,
        )),
    ]);
    ret = ret.intersection(&tmp);
    if !((a[2] <= s[3]) || (s[4] <= a[3]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 5 + -s[3] * 9 + s[2] * 4),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[3]) || (s[4] <= a[3]) || (s[2] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 45 / 29 + -s[3] * 61 / 29 + s[2] * 16 / 29),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[3]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[3] * 2 / 3 + s[2] * 3 / 2 + -s[0] * 5 / 6),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 8 / 37 + s[3] * 61 / 74 + -s[1] * 45 / 74),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(
                -a[3] * 8 / 37 + s[2] * 61 / 74 + -s[0] * 45 / 74,
            )),
            IntervalList::interval_lower(Bound::Excluded(
                -s[0] * 125 / 186 + -a[4] * 32 / 93 + s[2] * 125 / 186 + a[3] * 32 / 93,
            )),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[2]) || (a[4] <= s[4]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 9 + -s[2] * 25 / 9 + s[3] * 41 / 9),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !(s[2] <= a[1]) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[2] * 1 / 2 + -s[0] * 1 / 2),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[4]) || (a[0] <= s[2]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 4 + s[3] * 9 + -s[2] * 5),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[4]) || (a[0] <= s[1]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 2 / 3 + s[3] * 3 / 2 + -s[1] * 5 / 6),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_upper(Bound::Included(
                -a[3] * 8 / 37 + s[2] * 61 / 74 + -s[0] * 45 / 74,
            )),
            IntervalList::interval_lower(Bound::Included(
                -s[4] * 32 / 241 + s[2] * 369 / 482 + -s[0] * 305 / 482,
            )),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[2]) || (s[4] <= a[3]) || (s[1] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 25 / 14 + -s[2] * 61 / 14 + s[1] * 18 / 7),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[3]) || (s[4] <= a[3])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[4] + -s[3] * 1),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[1]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(s[2] + -s[1] * 1),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (a[0] <= s[1]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 13 + s[2] * 61 / 13 + -s[1] * 45 / 13),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (a[0] <= s[2]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 29 + -s[2] * 45 / 29 + s[3] * 61 / 29),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[0] <= s[2]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 64 / 241 + s[3] * 369 / 241 + -s[2] * 305 / 241),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[3]) || (s[4] <= a[2]) || (s[2] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 5 + -s[3] * 9 + s[2] * 4),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[0] <= s[1]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_upper(Bound::Included(
                -s[1] * 45 / 29 + -a[3] * 16 / 29 + s[2] * 61 / 29,
            )),
            IntervalList::interval_lower(Bound::Included(
                -s[4] * 64 / 177 + -s[1] * 305 / 177 + s[2] * 123 / 59,
            )),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[3]) || (a[0] <= s[1]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[3] * 4 + s[2] * 9 + -s[1] * 5),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[1] <= s[2]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 29 + -s[2] * 45 / 29 + s[3] * 61 / 29),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 59 + s[3] * 41 / 59 + -s[0] * 25 / 59),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[2]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Excluded(-s[2] * 1 + s[3]),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[0] <= s[1]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 8 / 17 + s[3] * 41 / 34 + -s[1] * 25 / 34),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !(s[4] <= a[3]) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[0] <= s[1]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(
                -s[1] * 45 / 29 + -a[3] * 16 / 29 + s[2] * 61 / 29,
            )),
            IntervalList::interval_lower(Bound::Included(
                -s[4] * 64 / 61 + -s[1] * 125 / 61 + a[3] * 64 / 61 + s[2] * 125 / 61,
            )),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 2 / 3 + s[3] * 3 / 2 + -s[1] * 5 / 6),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[2]) || (s[4] <= a[3]) || (s[1] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 45 / 74 + -s[2] * 61 / 74 + s[1] * 8 / 37),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[2]) || (s[4] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[1]) || (s[4] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[2] + -s[1] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !(s[4] <= a[1]) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[1] + -s[0] * 1)),
            IntervalList::interval_lower(Bound::Excluded(s[2] + -s[1] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[2]) || (s[4] <= a[2]) || (s[1] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 5 / 6 + -s[2] * 3 / 2 + s[1] * 2 / 3),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[3]) || (s[4] <= a[3]) || (s[1] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[4] * 45 / 13 + -s[3] * 61 / 13 + s[1] * 16 / 13),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (s[3] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 12 / 13 + s[3] * 61 / 39 + -s[0] * 25 / 39),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 8 / 29 + s[2] * 61 / 58 + -s[0] * 45 / 58),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !(s[4] <= a[2]) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[2] + -s[1] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[4]) || (s[2] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 2 + s[2] * 9 / 2 + -s[0] * 5 / 2),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[2] <= s[4]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 4 / 11 + s[3] * 9 / 11 + -s[0] * 5 / 11),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[3] <= s[4]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 16 / 119 + s[3] * 61 / 119 + -s[0] * 45 / 119),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 64 / 851 + s[3] * 369 / 851 + -s[0] * 305 / 851),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[4] <= s[4]) || (a[0] <= s[1]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(-s[4] * 32 / 273 + s[3] * 123 / 182 + -s[1] * 305 / 546),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[2]) || (s[4] <= a[2])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
            IntervalList::interval_lower(Bound::Excluded(s[4] + -s[3] * 1)),
        ]);
        ret = ret.intersection(&tmp);
    }
    if !((a[1] <= s[2]) || (s[3] <= a[2]) || (s[1] <= a[0])) {
        let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
            Bound::Included(s[3] * 5 + -s[2] * 9 + s[1] * 4),
        )]);
        ret = ret.intersection(&tmp);
    }
    if !((a[0] <= s[1]) || (s[3] <= a[1])) {
        let tmp = IntervalList::from_interval_lists(vec![
            IntervalList::interval_lower(Bound::Excluded(s[2] + -s[1] * 1)),
            IntervalList::interval_lower(Bound::Excluded(-s[2] * 1 + s[3])),
        ]);
        ret = ret.intersection(&tmp);
    }
    let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
        Bound::Excluded(RealNumRep::new(0, 1)),
    )]);
    ret = ret.intersection(&tmp);
    let tmp = IntervalList::from_interval_lists(vec![IntervalList::interval_lower(
        Bound::Included(s[4] + -s[3] * 1),
    )]);
    ret = ret.intersection(&tmp);
    ret
}
