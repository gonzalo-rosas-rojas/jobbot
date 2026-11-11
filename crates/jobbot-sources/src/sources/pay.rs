//! Pay that arrives as numbers plus a currency and a period (Himalayas, Jobicy), as US$ per month.
//! Only US dollars are converted: no exchange rate is ever guessed.

use serde_json::Value;

/// Working hours in a month (40 hours a week, 4 weeks), used for hourly rates.
const HOURS_PER_MONTH: f64 = 160.0;

/// A JSON number, or a string that holds one (`"4500"`). Anything else is `None`.
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// `amount` as US$ per month. `None` unless the currency is USD, the amount is positive and the
/// period is yearly (`year`, `annual`), monthly or hourly.
pub fn monthly_usd(amount: f64, currency: &str, period: &str) -> Option<f64> {
    if !amount.is_finite() || amount <= 0.0 || !currency.trim().eq_ignore_ascii_case("usd") {
        return None;
    }
    let period = period.to_ascii_lowercase();
    if period.contains("year") || period.contains("annual") {
        Some(amount / 12.0)
    } else if period.contains("month") {
        Some(amount)
    } else if period.contains("hour") {
        Some(amount * HOURS_PER_MONTH)
    } else {
        None
    }
}

/// `(min, max)` as US$ per month; each side is `None` when it cannot be converted.
pub fn monthly_usd_range(
    min: Option<f64>,
    max: Option<f64>,
    currency: &str,
    period: &str,
) -> (Option<f64>, Option<f64>) {
    let convert = |amount: Option<f64>| amount.and_then(|a| monthly_usd(a, currency, period));
    (convert(min), convert(max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_yearly_monthly_and_hourly_usd() {
        assert_eq!(monthly_usd(60_000.0, "USD", "annual"), Some(5_000.0));
        assert_eq!(monthly_usd(60_000.0, "usd", "yearly"), Some(5_000.0));
        assert_eq!(monthly_usd(3_500.0, "USD", "monthly"), Some(3_500.0));
        assert_eq!(monthly_usd(30.0, "USD", "hourly"), Some(4_800.0));
    }

    #[test]
    fn refuses_other_currencies_unknown_periods_and_bad_amounts() {
        assert_eq!(monthly_usd(60_000.0, "EUR", "annual"), None);
        assert_eq!(monthly_usd(60_000.0, "", "annual"), None);
        assert_eq!(monthly_usd(500.0, "USD", "weekly"), None);
        assert_eq!(monthly_usd(0.0, "USD", "annual"), None);
        assert_eq!(monthly_usd(-5.0, "USD", "annual"), None);
        assert_eq!(monthly_usd(f64::NAN, "USD", "annual"), None);
    }

    #[test]
    fn range_converts_each_side_on_its_own() {
        assert_eq!(
            monthly_usd_range(Some(60_000.0), Some(90_000.0), "USD", "annual"),
            (Some(5_000.0), Some(7_500.0))
        );
        assert_eq!(
            monthly_usd_range(Some(60_000.0), None, "USD", "annual"),
            (Some(5_000.0), None)
        );
        assert_eq!(
            monthly_usd_range(Some(1.0), Some(2.0), "CLP", "annual"),
            (None, None)
        );
    }

    #[test]
    fn number_reads_numbers_and_numeric_strings_only() {
        assert_eq!(number(&json!(4500)), Some(4500.0));
        assert_eq!(number(&json!(4500.5)), Some(4500.5));
        assert_eq!(number(&json!(" 4500 ")), Some(4500.0));
        assert_eq!(number(&json!("n/a")), None);
        assert_eq!(number(&json!(null)), None);
        assert_eq!(number(&json!([1])), None);
    }
}
