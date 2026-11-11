//! The readers of what job boards and their servers send, over generated input: dates in every
//! form a feed uses, `Retry-After` values, timestamps as JSON numbers, and the wait before a retry.
//! None of them may panic on anything a server sends, and each reads back what was written.

use std::time::Duration;

use chrono::{DateTime, FixedOffset, SecondsFormat, TimeDelta, TimeZone, Utc};
use proptest::prelude::*;
use serde_json::{Number, Value, json};

use jobbot_sources::sources::base::{
    RetryPolicy, backoff_delay, from_epoch, parse_iso, parse_retry_after, parse_rfc2822, parse_time,
};

/// The last second of year 9999: four-digit years are what the formats write.
const LAST_SECOND: i64 = 253_402_300_799;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 23, 12, 0, 0).unwrap()
}

/// Any moment from 1970 to the end of year 9999, to the nanosecond.
fn moment() -> impl Strategy<Value = DateTime<Utc>> {
    (0..=LAST_SECOND, 0u32..1_000_000_000)
        .prop_map(|(seconds, nanos)| DateTime::from_timestamp(seconds, nanos).unwrap())
}

/// A time zone a feed may write: any whole number of minutes from UTC, up to a day.
fn offset() -> impl Strategy<Value = FixedOffset> {
    (-1_439i32..=1_439).prop_map(|minutes| FixedOffset::east_opt(minutes * 60).unwrap())
}

fn padding() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just(""), Just(" "), Just("\t"), Just(" \n")]
}

proptest! {
    #[test]
    fn no_reader_panics_on_any_text(text in any::<String>(), seconds in any::<i64>()) {
        let _ = parse_iso(&text);
        let _ = parse_rfc2822(&text);
        let _ = parse_retry_after(&text, now());
        let _ = parse_time(&Value::String(text));
        let _ = from_epoch(seconds);
    }

    #[test]
    fn an_iso_time_reads_back_in_any_zone(
        at in moment(),
        zone in offset(),
        zulu in any::<bool>(),
        pad in padding(),
    ) {
        let written = if zulu {
            at.to_rfc3339_opts(SecondsFormat::AutoSi, true)
        } else {
            at.with_timezone(&zone).to_rfc3339()
        };
        prop_assert_eq!(parse_iso(&format!("{pad}{written}{pad}")), Some(at), "{}", written);
    }

    #[test]
    fn a_time_without_a_zone_is_read_as_utc(at in moment(), date_only in any::<bool>()) {
        let (written, expected) = if date_only {
            let midnight = at.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
            (at.format("%Y-%m-%d").to_string(), midnight)
        } else {
            (at.naive_utc().format("%Y-%m-%dT%H:%M:%S%.f").to_string(), at)
        };
        prop_assert_eq!(parse_iso(&written), Some(expected), "{}", written);
    }

    #[test]
    fn a_feed_date_reads_back(at in moment(), zone in offset()) {
        let at = at.with_nanosecond_zero();
        let written = at.with_timezone(&zone).to_rfc2822();
        prop_assert_eq!(parse_rfc2822(&written), Some(at), "{}", written);
    }

    #[test]
    fn retry_after_reads_any_number_of_seconds(seconds in any::<u64>(), pad in padding()) {
        prop_assert_eq!(
            parse_retry_after(&format!("{pad}{seconds}{pad}"), now()),
            Some(Duration::from_secs(seconds))
        );
    }

    #[test]
    fn retry_after_reads_any_http_date_and_a_past_one_is_no_wait(
        seconds in -100_000_000i64..100_000_000,
        gmt in any::<bool>(),
    ) {
        let when = now() + TimeDelta::seconds(seconds);
        let written = if gmt {
            when.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
        } else {
            when.to_rfc2822()
        };
        let expected = Duration::from_secs(seconds.max(0).unsigned_abs());
        prop_assert_eq!(parse_retry_after(&written, now()), Some(expected), "{}", written);
    }

    #[test]
    fn a_timestamp_as_any_json_number_is_read_as_unix_seconds_or_not_at_all(
        whole in any::<i64>(),
        big in any::<u64>(),
        fraction in proptest::num::f64::ANY,
    ) {
        prop_assert_eq!(parse_time(&json!(whole)), from_epoch(whole));
        prop_assert_eq!(parse_time(&json!(big)), i64::try_from(big).ok().and_then(from_epoch));
        if let Some(number) = Number::from_f64(fraction) {
            let read = parse_time(&Value::Number(number));
            if (0.0..LAST_SECOND as f64).contains(&fraction) {
                prop_assert_eq!(read, from_epoch(fraction as i64), "{}", fraction);
            }
        }
    }

    #[test]
    fn the_wait_before_a_retry_stays_within_the_policy_whatever_the_server_says(
        base_ms in 0u64..3_600_000,
        max_wait_ms in 0u64..864_000_000,
        attempt in any::<u32>(),
        retry_after in proptest::option::of((any::<u64>(), 0u32..1_000_000_000)),
        jitter in proptest::num::f64::ANY,
    ) {
        let policy = RetryPolicy {
            base: Duration::from_millis(base_ms),
            max_wait: Duration::from_millis(max_wait_ms),
            ..RetryPolicy::STANDARD
        };
        let retry_after = retry_after.map(|(seconds, nanos)| Duration::new(seconds, nanos));

        let wait = backoff_delay(&policy, attempt, retry_after, jitter);

        let asked = retry_after.unwrap_or_else(|| {
            policy
                .base
                .checked_mul(2u32.saturating_pow(attempt.min(10)))
                .unwrap_or(policy.max_wait)
        });
        prop_assert!(wait <= policy.max_wait, "{:?} over {:?}", wait, policy.max_wait);
        prop_assert!(
            wait >= asked.min(policy.max_wait),
            "jitter shortened the wait: {:?} under {:?}", wait, asked
        );
    }
}

trait NanosecondZero {
    fn with_nanosecond_zero(self) -> Self;
}

impl NanosecondZero for DateTime<Utc> {
    /// The same second: a feed date has no fraction.
    fn with_nanosecond_zero(self) -> Self {
        DateTime::from_timestamp(self.timestamp(), 0).unwrap()
    }
}
