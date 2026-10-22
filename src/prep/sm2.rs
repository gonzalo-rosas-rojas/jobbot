//! SM-2 spaced repetition (SuperMemo 2), the scheduling rule behind the interview drill.
//! The artifact panel runs the same rule in JavaScript; both read `tests/fixtures/sm2_vectors.json`.

/// One SM-2 step. Takes the card's ease factor, interval in days and number of successful
/// repetitions in a row, plus the grade Gonza gave himself (0 to 5), and returns the new
/// `(ease, interval, reps)`.
///
/// - A grade below 3 is a lapse: the card comes back tomorrow, the streak restarts, the ease stays.
/// - Otherwise the ease moves with the grade (never below 1.3) and the interval is 1 day after the
///   first success, 6 after the second, and `interval * ease` rounded half up after that.
///
/// Grades above 5 count as 5; callers validate the range before they get here.
pub fn sm2(ease: f64, interval: i64, reps: i64, grade: u8) -> (f64, i64, i64) {
    if grade < 3 {
        return (ease, 1, 0);
    }
    let miss = f64::from(5u8.saturating_sub(grade));
    let ease = (ease + 0.1 - miss * (0.08 + miss * 0.02)).max(1.3);
    let reps = reps + 1;
    let interval = match reps {
        1 => 1,
        2 => 6,
        // `f64::round` rounds halves away from zero, which for positive numbers is round half up.
        _ => (interval as f64 * ease).round() as i64,
    };
    (ease, interval, reps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_intervals_and_reset() {
        let (ease, interval, reps) = sm2(2.5, 0, 0, 5);
        assert_eq!((interval, reps), (1, 1));
        let (ease, interval, reps) = sm2(ease, interval, reps, 5);
        assert_eq!((interval, reps), (6, 2));
        let (kept_ease, interval, reps) = sm2(ease, interval, reps, 2);
        assert_eq!((interval, reps), (1, 0));
        assert_eq!(kept_ease, ease, "a lapse keeps the ease");
        assert!(sm2(1.3, 1, 1, 3).0 >= 1.3, "the ease never drops below 1.3");
    }

    #[test]
    fn third_success_multiplies_the_interval_by_the_ease() {
        let (ease, interval, reps) = sm2(2.7, 6, 2, 5);
        assert!((ease - 2.8).abs() < 1e-9);
        assert_eq!((interval, reps), (17, 3)); // 6 * 2.8 = 16.8 rounds to 17
    }
}
