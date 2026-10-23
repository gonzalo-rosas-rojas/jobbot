//! The application state machine: where an application is in its life and where it may go next.
//! Pure data and rules; the tracker (`jobbot::tracker`) stores the state and enforces these moves.

use std::fmt;

/// Where an application is in its life. Applications start at `Scored`; a
/// "found" state is implicit (a row in `jobs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum State {
    Scored,
    Prepared,
    NeedsGonza,
    Submitted,
    Replied,
    TechnicalTest,
    Interview,
    Offer,
    Accepted,
    Rejected,
    Withdrawn,
    Ghosted,
}

/// Every state reached only after a real submission.
pub const SUBMITTED_STATES: [State; 8] = [
    State::Submitted,
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
    State::Accepted,
    State::Rejected,
    State::Ghosted,
];

impl State {
    pub const ALL: [State; 12] = [
        State::Scored,
        State::Prepared,
        State::NeedsGonza,
        State::Submitted,
        State::Replied,
        State::TechnicalTest,
        State::Interview,
        State::Offer,
        State::Accepted,
        State::Rejected,
        State::Withdrawn,
        State::Ghosted,
    ];

    /// The name stored in the database and shown in messages.
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Scored => "scored",
            State::Prepared => "prepared",
            State::NeedsGonza => "needs_gonza",
            State::Submitted => "submitted",
            State::Replied => "replied",
            State::TechnicalTest => "technical_test",
            State::Interview => "interview",
            State::Offer => "offer",
            State::Accepted => "accepted",
            State::Rejected => "rejected",
            State::Withdrawn => "withdrawn",
            State::Ghosted => "ghosted",
        }
    }

    pub fn parse(s: &str) -> Option<State> {
        State::ALL.into_iter().find(|state| state.as_str() == s)
    }

    /// The states this one may move to. The terminal states have none.
    pub fn allowed_next(&self) -> &'static [State] {
        use State::*;
        match self {
            Scored => &[Prepared, NeedsGonza, Withdrawn],
            Prepared => &[Submitted, NeedsGonza, Withdrawn],
            // `Submitted` only through `jobbot confirm`: Gonza verified that an unconfirmed
            // submission went through.
            NeedsGonza => &[Scored, Prepared, Submitted, Withdrawn],
            Submitted => &[
                Replied,
                TechnicalTest,
                Interview,
                Rejected,
                Ghosted,
                Withdrawn,
            ],
            Replied => &[TechnicalTest, Interview, Rejected, Withdrawn, Ghosted],
            TechnicalTest => &[Interview, Rejected, Withdrawn, Ghosted],
            Interview => &[Offer, Rejected, Withdrawn, Ghosted, Interview],
            Offer => &[Accepted, Rejected, Withdrawn],
            // Silence is not the end: a late answer can be a test, an offer or a rejection too.
            Ghosted => &[Replied, TechnicalTest, Interview, Offer, Rejected],
            Accepted | Rejected | Withdrawn => &[],
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_names_round_trip() {
        for state in State::ALL {
            assert_eq!(State::parse(state.as_str()), Some(state));
            assert_eq!(state.to_string(), state.as_str());
        }
        assert_eq!(State::parse("nope"), None);
        assert_eq!(State::ALL.len(), 12);
    }

    #[test]
    fn transition_table_matches_the_spec() {
        use State::*;
        assert_eq!(Scored.allowed_next(), &[Prepared, NeedsGonza, Withdrawn]);
        assert_eq!(
            NeedsGonza.allowed_next(),
            &[Scored, Prepared, Submitted, Withdrawn]
        );
        assert_eq!(
            Submitted.allowed_next(),
            &[
                Replied,
                TechnicalTest,
                Interview,
                Rejected,
                Ghosted,
                Withdrawn
            ]
        );
        assert!(Interview.allowed_next().contains(&Interview));
        // Silence is not the end: a late answer can still be a test, an offer or a rejection.
        assert_eq!(
            Ghosted.allowed_next(),
            &[Replied, TechnicalTest, Interview, Offer, Rejected]
        );
        for terminal in [Accepted, Rejected, Withdrawn] {
            assert!(terminal.allowed_next().is_empty());
        }
    }

    #[test]
    fn submitted_states_need_a_real_submission() {
        assert_eq!(SUBMITTED_STATES.len(), 8);
        for state in [
            State::Scored,
            State::Prepared,
            State::NeedsGonza,
            State::Withdrawn,
        ] {
            assert!(!SUBMITTED_STATES.contains(&state));
        }
        assert!(
            State::Scored
                .allowed_next()
                .iter()
                .all(|next| !SUBMITTED_STATES.contains(next))
        );
        // `needs_gonza -> submitted` exists for `jobbot confirm`: Gonza saw on the site, or in
        // his inbox, that a submission the program could not confirm did go through.
        assert!(State::NeedsGonza.allowed_next().contains(&State::Submitted));
    }
}
