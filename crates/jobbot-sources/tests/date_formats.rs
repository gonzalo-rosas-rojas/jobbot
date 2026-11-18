//! How the posting dates the job boards send are read, written out as literals: ISO 8601 and
//! RFC 3339 with every fraction and zone, RFC 2822 (the RSS `pubDate`), the HTTP date of
//! `Retry-After`, and Unix seconds in a JSON number. `properties.rs` checks that anything the
//! library writes reads back; these say what each text a real feed sends means, and which texts
//! are refused, so the reading stays the same when the library behind it changes.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use jobbot_sources::sources::base::{
    from_epoch, parse_iso, parse_retry_after, parse_rfc2822, parse_time,
};
use serde_json::json;

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

fn nanos(at: DateTime<Utc>, nanos: i64) -> DateTime<Utc> {
    at + TimeDelta::nanoseconds(nanos)
}

#[test]
fn an_iso_time_is_read_with_zulu_an_offset_or_no_zone_at_all() {
    let noon = utc(2026, 9, 30, 16, 0, 0);
    for text in [
        "2026-09-30T16:00:00Z",
        "2026-09-30T16:00:00z",
        "2026-09-30T16:00:00+00:00",
        "2026-09-30T12:00:00-04:00",
        "2026-09-30T21:30:00+05:30",
        "2026-09-30 16:00:00Z",
        "2026-09-30T16:00:00",
        "  2026-09-30T16:00:00Z\n",
    ] {
        assert_eq!(parse_iso(text), Some(noon), "{text:?}");
    }
    assert_eq!(
        parse_iso("2026-09-30T16:00:00+23:59"),
        Some(utc(2026, 9, 29, 16, 1, 0))
    );
}

#[test]
fn an_iso_time_keeps_the_fraction_up_to_nanoseconds_and_drops_the_digits_after_them() {
    let noon = utc(2026, 9, 30, 16, 0, 0);
    for (text, extra) in [
        ("2026-09-30T16:00:00.5+00:00", 500_000_000),
        ("2026-09-30T16:00:00.737Z", 737_000_000),
        ("2026-09-30T16:00:00.123456Z", 123_456_000),
        ("2026-09-30T16:00:00.123456789Z", 123_456_789),
        ("2026-09-30T16:00:00.1234567891Z", 123_456_789),
        ("2026-09-30T16:00:00.123", 123_000_000),
    ] {
        assert_eq!(parse_iso(text), Some(nanos(noon, extra)), "{text:?}");
    }
}

#[test]
fn an_iso_date_alone_is_midnight_utc_and_other_calendar_dates_are_checked() {
    assert_eq!(parse_iso("2026-09-30"), Some(utc(2026, 9, 30, 0, 0, 0)));
    assert_eq!(parse_iso("2026-9-30"), Some(utc(2026, 9, 30, 0, 0, 0)));
    assert_eq!(parse_iso("2028-02-29"), Some(utc(2028, 2, 29, 0, 0, 0)));
    assert_eq!(parse_iso("2026-02-29"), None, "2026 has no February 29");
    assert_eq!(
        parse_iso("0000-01-01T00:00:00Z"),
        Some(utc(0, 1, 1, 0, 0, 0)),
        "year 0 is a date"
    );
    assert_eq!(
        parse_iso("10000-01-01T00:00:00Z"),
        None,
        "five-digit years are not"
    );
}

#[test]
fn a_leap_second_is_read_as_its_own_second() {
    let read = parse_iso("2016-12-31T23:59:60Z").expect("a leap second is a time");
    assert_eq!(read.timestamp(), 1_483_228_799);
}

#[test]
fn an_iso_time_that_is_not_one_is_refused() {
    for text in [
        "",
        "   ",
        "yesterday",
        "1759320000",
        "20260930",
        "2026-09-30T",
        "2026-09-30T16:00Z",
        "2026-09-30T16:00:00+0000",
        "2026-09-30T16:00:00+24:00",
        "2026-09-30T16:00:00.Z",
        "2026-09-30T16:00:00 UTC",
        "2026-09-30T24:00:00Z",
        "2026-09-30 16:00:00",
    ] {
        assert_eq!(parse_iso(text), None, "{text:?}");
    }
}

#[test]
fn a_feed_date_is_read_with_a_numeric_zone_gmt_or_a_us_zone_name() {
    let seven_thirty = utc(2026, 10, 4, 7, 31, 8);
    for text in [
        "Sun, 04 Oct 2026 07:31:08 +0000",
        "Sun, 04 Oct 2026 03:31:08 -0400",
        "Sun, 04 Oct 2026 07:31:08 GMT",
        "Sun, 04 Oct 2026 07:31:08 UT",
        "Sun, 04 Oct 2026 07:31:08 Z",
        "Sun, 4 Oct 2026 07:31:08 +0000",
        "4 Oct 2026 07:31:08 +0000",
        "sun, 04 oct 2026 07:31:08 +0000",
        "Sun, 04 Oct 26 07:31:08 +0000",
        "Sun, 04 Oct 2026 07:31:08 +0000 (UTC)",
        "  Sun, 04 Oct 2026 07:31:08 +0000\n",
    ] {
        assert_eq!(parse_rfc2822(text), Some(seven_thirty), "{text:?}");
    }
    assert_eq!(
        parse_rfc2822("Sun, 04 Oct 2026 07:31:08 EST"),
        Some(utc(2026, 10, 4, 12, 31, 8)),
        "the old zone names mean what they meant: EST is five hours behind"
    );
    assert_eq!(
        parse_rfc2822("Sun, 04 Oct 2026 07:31:08 PDT"),
        Some(utc(2026, 10, 4, 14, 31, 8))
    );
    assert_eq!(
        parse_rfc2822("Sun, 04 Oct 2026 07:31 +0000"),
        Some(utc(2026, 10, 4, 7, 31, 0)),
        "the seconds may be left out"
    );
}

#[test]
fn a_feed_date_whose_weekday_or_shape_is_wrong_is_refused() {
    for text in [
        "",
        "Mon, 04 Oct 2026 07:31:08 +0000",
        "Sat, 29 Feb 2028 12:00:00 +0000",
        "Wed, 29 Feb 2023 12:00:00 +0000",
        "Sun, 04 Oct 2026 07:31:08 +00:00",
        "Sun, 04 Oct 2026 07:31:08",
        "Sun, 04 Oct 2026 07:31:08.250 +0000",
        "Sun, 04 Oct 2026 24:00:00 +0000",
        "Sunday, 04-Oct-26 07:31:08 GMT",
        "Sun Oct  4 07:31:08 2026",
        "2026-10-04T07:31:08Z",
    ] {
        assert_eq!(parse_rfc2822(text), None, "{text:?}");
    }
    assert_eq!(
        parse_rfc2822("Tue, 29 Feb 2028 12:00:00 +0000"),
        Some(utc(2028, 2, 29, 12, 0, 0)),
        "a real leap day with its real weekday"
    );
}

#[test]
fn unix_seconds_cover_the_whole_range_the_library_can_hold() {
    assert_eq!(from_epoch(0), Some(utc(1970, 1, 1, 0, 0, 0)));
    assert_eq!(from_epoch(-1), Some(utc(1969, 12, 31, 23, 59, 59)));
    assert_eq!(from_epoch(1_759_320_000), Some(utc(2025, 10, 1, 12, 0, 0)));
    assert_eq!(
        from_epoch(253_402_300_799),
        Some(utc(9999, 12, 31, 23, 59, 59))
    );
    assert_eq!(from_epoch(-62_135_596_800), Some(utc(1, 1, 1, 0, 0, 0)));
    assert_eq!(from_epoch(i64::MAX), None);
    assert_eq!(from_epoch(i64::MIN), None);
}

#[test]
fn known_oddity_milliseconds_mistaken_for_seconds_are_a_date_in_the_year_57720_not_a_refusal() {
    // The change of date library may refuse this value on purpose (ADR 0015); then update this test.
    // A board that sends milliseconds where seconds are expected is not caught by the reader: it
    // gets a posting dated 55,000 years ahead. (Lever sends milliseconds and divides them itself.)
    use chrono::Datelike;
    let read = from_epoch(1_759_320_000_000).expect("the library holds that far");
    assert_eq!(read.year(), 57720);
    assert_eq!(
        from_epoch(253_402_300_800).map(|at| at.year()),
        Some(10_000)
    );
    assert_eq!(
        parse_time(&json!(1_759_320_000_000_i64)).map(|at| at.year()),
        Some(57720)
    );
}

#[test]
fn a_json_time_is_unix_seconds_cut_to_the_second_or_an_iso_string() {
    let noon = utc(2025, 10, 1, 12, 0, 0);
    assert_eq!(parse_time(&json!(1_759_320_000)), Some(noon));
    assert_eq!(
        parse_time(&json!(1_759_320_000.5)),
        Some(noon),
        "the half second is cut off"
    );
    assert_eq!(parse_time(&json!(1_759_320_000.999)), Some(noon));
    assert_eq!(
        parse_time(&json!(-1.5)),
        Some(utc(1969, 12, 31, 23, 59, 59)),
        "a negative fraction is cut toward zero"
    );
    assert_eq!(
        parse_time(&json!("2025-10-01T12:00:00.250Z")),
        Some(nanos(noon, 250_000_000))
    );
    assert_eq!(
        parse_time(&json!("1759320000")),
        None,
        "digits in a string are not seconds"
    );
    assert_eq!(parse_time(&json!(1e20)), None);
    assert_eq!(parse_time(&json!(u64::MAX)), None);
    assert_eq!(parse_time(&json!(true)), None);
}

#[test]
fn a_retry_after_is_seconds_or_an_http_date_in_gmt_and_other_http_dates_are_no_answer() {
    let now = utc(2026, 10, 21, 7, 27, 0);
    assert_eq!(
        parse_retry_after("120", now),
        Some(Duration::from_secs(120))
    );
    assert_eq!(
        parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT", now),
        Some(Duration::from_secs(60)),
        "the form the HTTP standard names first"
    );
    assert_eq!(
        parse_retry_after("Wed, 21 Oct 2026 07:28:30 +0000", now),
        Some(Duration::from_secs(90))
    );
    assert_eq!(
        parse_retry_after("Wed, 21 Oct 2026 07:20:00 GMT", now),
        Some(Duration::ZERO),
        "a date already past is no wait"
    );
    assert_eq!(
        parse_retry_after("Wed, 28 Oct 2026 07:27:00 GMT", now),
        Some(Duration::from_secs(7 * 24 * 3600))
    );
    // The two older HTTP date forms are not read: the wait falls back to the policy's own.
    assert_eq!(
        parse_retry_after("Sunday, 06-Nov-94 08:49:37 GMT", now),
        None
    );
    assert_eq!(parse_retry_after("Sun Nov  6 08:49:37 1994", now), None);
}
