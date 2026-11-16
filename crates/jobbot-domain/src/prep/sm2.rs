//! SM-2 spaced repetition (SuperMemo 2), the scheduling rule behind the interview drill.
//! `tests/sm2_vectors.rs` checks it against `tests/fixtures/sm2_vectors.json`; any other
//! implementation of the rule (the panel has none today) must read the same file.

/// The longest interval a card is ever given: ten years. Without a cap a card graded well over and
/// over multiplies its interval into a number of days no date can hold.
pub const MAX_INTERVAL_DAYS: i64 = 3650;

/// Ease factor of a card that has never been graded (SM-2's starting value).
pub const INITIAL_EASE: f64 = 2.5;

/// The ease factor never drops below this (SM-2's floor).
const MIN_EASE: f64 = 1.3;

/// One SM-2 step. Takes the card's ease factor, interval in days and number of successful
/// repetitions in a row, plus the grade Gonza gave himself (0 to 5), and returns the new
/// `(ease, interval, reps)`.
///
/// - A grade below 3 is a lapse: the card comes back tomorrow, the streak restarts, the ease stays.
/// - Otherwise the ease moves with the grade (never below `MIN_EASE`, 1.3) and the interval is 1 day after the
///   first success, 6 after the second, and `interval * ease` rounded half up after that.
///
/// The interval never exceeds `MAX_INTERVAL_DAYS`.
///
/// Grades above 5 count as 5; callers validate the range before they get here.
pub fn sm2(ease: f64, interval: i64, reps: i64, grade: u8) -> (f64, i64, i64) {
    if grade < 3 {
        return (ease, 1, 0);
    }
    let miss = f64::from(5u8.saturating_sub(grade));
    // SuperMemo 2: EF' = EF + (0.1 - m * (0.08 + m * 0.02)), with m = 5 - grade.
    let ease = (ease + 0.1 - miss * (0.08 + miss * 0.02)).max(MIN_EASE);
    let reps = reps + 1;
    let interval = match reps {
        1 => 1,
        2 => 6,
        // `f64::round` rounds halves away from zero, which for positive numbers is round half up.
        _ => (interval as f64 * ease).round() as i64,
    }
    .min(MAX_INTERVAL_DAYS);
    (ease, interval, reps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_intervals_and_reset() {
        let (ease, interval, reps) = sm2(INITIAL_EASE, 0, 0, 5);
        assert_eq!((interval, reps), (1, 1));
        let (ease, interval, reps) = sm2(ease, interval, reps, 5);
        assert_eq!((interval, reps), (6, 2));
        let (kept_ease, interval, reps) = sm2(ease, interval, reps, 2);
        assert_eq!((interval, reps), (1, 0));
        assert_eq!(kept_ease, ease, "a lapse keeps the ease");
        assert!(
            sm2(MIN_EASE, 1, 1, 3).0 >= MIN_EASE,
            "the ease never drops below the floor"
        );
    }

    #[test]
    fn the_interval_stops_growing_at_ten_years() {
        let (mut ease, mut interval, mut reps) = (2.5, 0, 0);
        for _ in 0..200 {
            (ease, interval, reps) = sm2(ease, interval, reps, 5);
            assert!(interval <= MAX_INTERVAL_DAYS);
        }
        assert_eq!(interval, MAX_INTERVAL_DAYS);
        // A card stored with an interval beyond the cap is brought back under it.
        assert_eq!(sm2(2.5, i64::MAX, 9, 5).1, MAX_INTERVAL_DAYS);
    }

    #[test]
    fn third_success_multiplies_the_interval_by_the_ease() {
        let (ease, interval, reps) = sm2(2.7, 6, 2, 5);
        assert!((ease - 2.8).abs() < 1e-9);
        assert_eq!((interval, reps), (17, 3)); // 6 * 2.8 = 16.8 rounds to 17
    }
}
