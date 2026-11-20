//! How an application came about, the one fact the funnel by origin groups by.
//!
//! Pure data and names. The tracker stores the name in `applications.origin`; the actions parse what
//! Gonza typed.

use std::fmt;

/// How an application came about. Stored in `applications.origin` by its `as_str` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    /// jobbot found the posting and applied without any contact.
    Cold,
    /// Someone inside the company referred him.
    Referral,
    /// A recruiter contacted him first.
    Recruiter,
    /// Through a staffing or recruiting agency.
    Agency,
    /// Through his profile on a talent platform.
    Platform,
    /// He wrote to someone at the company himself.
    Direct,
    /// Nobody wrote down how it came about.
    Unknown,
}

impl Origin {
    pub const ALL: [Origin; 7] = [
        Origin::Cold,
        Origin::Referral,
        Origin::Recruiter,
        Origin::Agency,
        Origin::Platform,
        Origin::Direct,
        Origin::Unknown,
    ];

    /// The name stored in the database and shown in messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Cold => "cold",
            Origin::Referral => "referral",
            Origin::Recruiter => "recruiter",
            Origin::Agency => "agency",
            Origin::Platform => "platform",
            Origin::Direct => "direct",
            Origin::Unknown => "unknown",
        }
    }

    /// The origin whose stored name is exactly `text`; `None` for anything else (no trimming, no case
    /// folding: the store reads it back with this, and callers that take typed text normalise it first).
    pub fn parse(text: &str) -> Option<Origin> {
        Origin::ALL.into_iter().find(|origin| origin.as_str() == text)
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_names_round_trip() {
        for origin in Origin::ALL {
            assert_eq!(Origin::parse(origin.as_str()), Some(origin));
            assert_eq!(origin.to_string(), origin.as_str());
        }
    }

    #[test]
    fn origin_names_are_the_seven_of_the_decision() {
        assert_eq!(
            Origin::ALL.map(Origin::as_str),
            [
                "cold",
                "referral",
                "recruiter",
                "agency",
                "platform",
                "direct",
                "unknown"
            ]
        );
    }

    #[test]
    fn parse_takes_only_the_exact_stored_name() {
        for text in ["", "Cold", " cold", "frio", "referido"] {
            assert_eq!(Origin::parse(text), None, "{text:?}");
        }
    }

    /// Position of each origin in `Origin::ALL`. The match has no `_` arm, so a new variant does not
    /// compile until it is listed here, and the test below then checks `ALL` against it.
    fn position_in_all(origin: Origin) -> usize {
        match origin {
            Origin::Cold => 0,
            Origin::Referral => 1,
            Origin::Recruiter => 2,
            Origin::Agency => 3,
            Origin::Platform => 4,
            Origin::Direct => 5,
            Origin::Unknown => 6,
        }
    }

    #[test]
    fn every_origin_is_listed_in_all_once() {
        for (index, origin) in Origin::ALL.into_iter().enumerate() {
            assert_eq!(position_in_all(origin), index, "{origin}");
        }
        assert_eq!(
            Origin::ALL.len(),
            position_in_all(Origin::Unknown) + 1,
            "ALL has an origin the position table lacks, or misses one"
        );
    }
}
