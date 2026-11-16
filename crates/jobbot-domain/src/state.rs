//! The application state machine: where an application is in its life and where it may go next.
//! Pure data and rules; the tracker (`jobbot::tracker`) stores the state and enforces these moves.

use std::fmt;

/// Where an application is in its life. Applications start at `Scored`; a posting that was only
/// found has no state of its own: it is a row in `jobs`.
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

/// The states that mean an application got an answer (a rejection is one). The weekly digest and
/// the panel's metrics and funnel count these, so they never disagree about what "answered" is.
pub const ANSWERED_STATES: [State; 5] = [
    State::Replied,
    State::TechnicalTest,
    State::Interview,
    State::Offer,
    State::Rejected,
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
    }

    /// Position of each state in `State::ALL`. The match has no `_` arm, so a new state does not
    /// compile until it is listed here, and the test below then checks `ALL` against it.
    fn position_in_all(state: State) -> usize {
        match state {
            State::Scored => 0,
            State::Prepared => 1,
            State::NeedsGonza => 2,
            State::Submitted => 3,
            State::Replied => 4,
            State::TechnicalTest => 5,
            State::Interview => 6,
            State::Offer => 7,
            State::Accepted => 8,
            State::Rejected => 9,
            State::Withdrawn => 10,
            State::Ghosted => 11,
        }
    }

    #[test]
    fn all_lists_every_state_once_in_order() {
        for (index, state) in State::ALL.into_iter().enumerate() {
            assert_eq!(position_in_all(state), index, "{state}");
        }
        assert_eq!(
            State::ALL.len(),
            position_in_all(State::Ghosted) + 1,
            "ALL has a state the position table lacks, or misses one"
        );
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
