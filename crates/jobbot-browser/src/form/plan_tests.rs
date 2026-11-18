//! The plan of a fill (`plan_fields` and its companions, which live in `jobbot_domain::form`)
//! against hand-built controls and answer banks, and once against the bank the repo ships.

use std::path::PathBuf;

use super::support_tests::{
    action_idxs, bank_with, bank_with_gaps, checkbox, checkbox_group, field, pay_and_start_bank,
    race_bank, radio, texts,
};
use super::*;
use jobbot_domain::answers::AnswerEntry;

#[test]
fn plan_fills_text_and_select_fields_from_the_bank() {
    let fields = [
        field(0, "input", "text", "First name *", true),
        field(1, "select", "", "Country", false),
    ];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
    assert_eq!(
        plan.actions,
        vec![
            FillAction {
                idx: 0,
                label: "First name *".into(),
                value: FillValue::Text("Jane".into())
            },
            FillAction {
                idx: 1,
                label: "Country".into(),
                value: FillValue::Select(vec!["Chile".into()])
            },
        ]
    );
}

#[test]
fn select_candidates_include_alternatives() {
    let fields = [field(0, "select", "", "How did you hear about us?", true)];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(
        plan.actions[0].value,
        FillValue::Select(vec![
            "Company careers page".into(),
            "Company website".into(),
            "Other".into()
        ])
    );
    // A textarea is an essay box: a keyword pattern ("how did you hear") does not answer it,
    // only an entry written for its whole label does, and then it gets only the value:
    // alternatives are option texts, not free text. (A one-line input may be a custom
    // drop-down and keeps them: see `a_text_input_with_alternatives_may_be_a_custom_dropdown`.)
    let fields = [field(0, "textarea", "", "How did you hear about us?", true)];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty());
    assert_eq!(plan.missing, vec!["How did you hear about us?"]);
    let exact = bank_with(vec![
        AnswerEntry::new(
            &["=how did you hear about us"],
            Some("Company careers page"),
        )
        .with_alternatives(&["Company website", "Other"]),
    ]);
    let plan = plan_fields(&fields, &exact, Path::new("cv.pdf"));
    assert_eq!(
        plan.actions[0].value,
        FillValue::Text("Company careers page".into())
    );
}

#[test]
fn consent_checkbox_is_ticked_only_when_allowed() {
    let fields = [
        field(
            0,
            "input",
            "checkbox",
            "I have read and accept the Privacy Policy *",
            true,
        ),
        field(1, "input", "checkbox", "Send me job alerts", false),
    ];
    // The shipped default: nothing is allowed, so the required checkbox is a missing answer.
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(
        plan.missing,
        vec!["I have read and accept the Privacy Policy *"]
    );
    assert!(plan.actions.is_empty());
    // With Gonza's phrase the required checkbox is ticked; the optional one is left alone.
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy policy".into()],
        ..bank_with_gaps()
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
    assert_eq!(
        plan.actions,
        vec![FillAction {
            idx: 0,
            label: "I have read and accept the Privacy Policy *".into(),
            value: FillValue::Check,
        }]
    );
}

#[test]
fn radio_group_is_answered_from_the_bank() {
    let fields = [
        radio(
            0,
            "sponsor",
            "Do you require visa sponsorship? *",
            "Yes",
            true,
        ),
        radio(
            1,
            "sponsor",
            "Do you require visa sponsorship? *",
            "No",
            true,
        ),
        radio(2, "relocate", "Willing to relocate?", "Yes", false),
        radio(3, "relocate", "Willing to relocate?", "No", false),
        radio(
            4,
            "sponsor2",
            "Will you now or in the future require sponsorship?",
            "Yes, I will need sponsorship",
            true,
        ),
        radio(
            5,
            "sponsor2",
            "Will you now or in the future require sponsorship?",
            "No, I will not need sponsorship",
            true,
        ),
    ];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
    // One button per answered group ("No" also matches "No, ..."); the optional group with no
    // bank answer is skipped.
    assert_eq!(
        plan.actions,
        vec![
            FillAction {
                idx: 1,
                label: "Do you require visa sponsorship? *".into(),
                value: FillValue::Check
            },
            FillAction {
                idx: 5,
                label: "Will you now or in the future require sponsorship?".into(),
                value: FillValue::Check,
            },
        ]
    );
}

#[test]
fn an_option_that_already_shows_the_answer_is_not_clicked_again() {
    // A pressed toggle button released by a second click would undo the answer, and a box the
    // page already ticked needs nothing: the question is answered either way.
    let option = |idx: usize, text: &str, checked: bool| FieldInfo {
        checked,
        ..radio(
            idx,
            "sponsor",
            "Do you require visa sponsorship? *",
            text,
            true,
        )
    };
    let unanswered = field(2, "input", "text", "Favorite color *", true);
    let fields = [option(0, "Yes", false), option(1, "No", true), unanswered];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing, vec!["Favorite color *"]);
    // Only the open question is left for Gonza, not the group that already says "No".
    assert_eq!(controls_left(&fields, &plan, &[]), vec![2]);
    // Another option selected: the bank's answer is clicked.
    let fields = [option(0, "Yes", true), option(1, "No", false)];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].idx, 1);
}

#[test]
fn plan_uploads_the_cv_only_to_resume_inputs() {
    let fields = [
        field(0, "input", "file", "Resume/CV *", true),
        field(1, "input", "file", "Cover letter", false),
        field(2, "input", "file", "Currículum", false),
    ];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert!(plan.missing.is_empty());
    let uploaded = action_idxs(&plan);
    assert_eq!(uploaded, vec![0, 2]);
    assert_eq!(
        plan.actions[0].value,
        FillValue::File(PathBuf::from("cv.pdf"))
    );
}

#[test]
fn required_file_input_that_is_not_a_resume_is_missing() {
    let fields = [field(0, "input", "file", "Portfolio PDF", true)];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["Portfolio PDF"]);
    assert!(plan.actions.is_empty());
}

#[test]
fn required_field_without_a_value_is_missing_and_optional_one_is_skipped() {
    let fields = [
        field(0, "input", "email", "Email *", true),
        field(1, "input", "text", "Favorite color", true),
        field(2, "input", "text", "Nickname", false),
        field(3, "input", "text", "", true),
    ];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(
        plan.missing,
        vec!["Email *", "Favorite color", "campo sin etiqueta"]
    );
    assert!(plan.actions.is_empty());
}

#[test]
fn blank_bank_value_counts_as_missing() {
    let fields = [field(0, "input", "tel", "Phone", true)];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["Phone"]);
}

#[test]
fn a_date_input_never_gets_the_word_immediately() {
    // A date input rejects free text: typing "Immediately" would leave it empty or throw
    // halfway through the form. The question is Gonza's, with a real date.
    let fields = [
        field(0, "input", "date", "Earliest start date *", true),
        field(1, "input", "month", "Start", false),
        field(2, "input", "datetime-local", "Start", true),
        field(3, "input", "text", "Start", true),
    ];
    let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["Earliest start date *", "Start"]);
    assert_eq!(
        plan.actions,
        vec![FillAction {
            idx: 3,
            label: "Start".into(),
            value: FillValue::Text("Immediately".into())
        }]
    );
}

#[test]
fn a_number_input_only_gets_a_number() {
    let fields = [
        field(0, "input", "number", "Salary", true),
        field(1, "input", "number", "Annual", true),
        field(2, "input", "number", "Salary", false),
    ];
    let plan = plan_fields(&fields, &pay_and_start_bank(), Path::new("cv.pdf"));
    // The monthly text cannot go in a numeric box (required: Gonza answers; optional: skipped);
    // the annual figure can.
    assert_eq!(plan.missing, vec!["Salary"]);
    assert_eq!(
        plan.actions,
        vec![FillAction {
            idx: 1,
            label: "Annual".into(),
            value: FillValue::Text("72000".into())
        }]
    );
}

#[test]
fn no_date_or_time_box_ever_gets_free_text() {
    let fields = |required: bool| {
        ["date", "month", "week", "time", "datetime-local"]
            .iter()
            .enumerate()
            .map(|(idx, kind)| field(idx, "input", kind, "Start", required))
            .collect::<Vec<_>>()
    };
    // Required: the question is Gonza's. Optional: left alone. Never "Immediately".
    let plan = plan_fields(&fields(true), &pay_and_start_bank(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing, vec!["Start"]);
    let plan = plan_fields(&fields(false), &pay_and_start_bank(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert!(plan.missing.is_empty());
}

#[test]
fn a_date_picker_stays_unanswered_whatever_text_the_scan_gives_it() {
    // Ashby's start date is a text input drawn as a date picker. Its label used to be the
    // placeholder ("Pick date..."), now it is the question; the shipped bank answers that
    // question with "Immediately", which a date picker cannot take, so Gonza picks the day.
    let shipped = crate::test_support::shipped_bank();
    for label in ["When can you start a new role?", "Pick date...", ""] {
        let fields = [field(0, "input", "date", label, true)];
        let plan = plan_fields(&fields, &shipped, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{label:?}: {:?}", plan.actions);
        assert_eq!(
            plan.missing,
            vec![label_or(label, "campo sin etiqueta")],
            "{label:?}"
        );
    }
}

#[test]
fn a_control_of_an_unknown_kind_is_never_typed_into_and_is_reported() {
    // A kind this planner does not know (a color picker, a password box, something a later
    // version of the scan invents) must not be skipped quietly or fed text: a required one is
    // reported whatever the bank says, an optional one is left untouched.
    let fields = [
        field(0, "input", "color", "Brand color *", true),
        field(1, "input", "hologram", "Mystery *", true),
        field(2, "input", "password", "Choose a password *", true),
        field(3, "input", "hologram", "Another mystery", false),
    ];
    let bank = bank_with(vec![
        AnswerEntry::new(&["brand color"], Some("Blue")),
        AnswerEntry::new(&["mystery"], Some("42")),
        AnswerEntry::new(&["password"], Some("hunter2")),
    ]);
    for bank in [&bank, &AnswerBank::default()] {
        let plan = plan_fields(&fields, bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(
            plan.missing,
            vec!["Brand color *", "Mystery *", "Choose a password *"]
        );
    }
    // The kinds that do take text are still filled.
    let known: Vec<FieldInfo> = ["", "text", "search", "email", "tel", "url", "combobox"]
        .iter()
        .enumerate()
        .map(|(idx, kind)| field(idx, "input", kind, "Mystery *", true))
        .collect();
    let plan = plan_fields(&known, &bank, Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), known.len());
    assert!(plan.missing.is_empty());
}

#[test]
fn every_spelling_of_not_hispanic_is_refused() {
    for option in [
        "Not Hispanic or Latino",
        "White (Not Hispanic or Latino)",
        "White, non-Hispanic",
        "Non-Hispanic White",
        "No, I am not Hispanic or Latino",
        "Not of Hispanic or Latino origin",
        "White (not of Hispanic origin)",
        "Not Latino",
        "Non-Latino White",
        "I am not of Latino descent",
        "White (Other than Hispanic or Latino)",
        "White, excluding Hispanic or Latino",
        "White (No Hispanic origin)",
        "White except Hispanic",
        "White without Hispanic or Latino origin",
        "Caucasian (other than Latinx)",
        "Black or African American (Excluding Latina)",
    ] {
        assert!(claims_not_hispanic(option), "{option}");
    }
    for option in [
        "Hispanic or Latino",
        "Hispanic",
        "White",
        "Decline to self-identify",
        "Other",
        "Other (please specify)",
        "No answer",
        "I do not wish to answer",
        "Prefer not to say",
        "Hispanic or Latino (any race)",
    ] {
        assert!(!claims_not_hispanic(option), "{option}");
    }
}

#[test]
fn an_option_that_says_not_hispanic_is_never_picked_for_gonza() {
    // Ashby's race list has no plain "White": its option is "White (Not Hispanic or Latino)",
    // which also answers the Hispanic/Latino question Gonza keeps for himself.
    let group = |required: bool| {
        [
            radio(0, "race", "Race", "Hispanic or Latino", required),
            radio(
                1,
                "race",
                "Race",
                "White (Not Hispanic or Latino)",
                required,
            ),
            radio(
                2,
                "race",
                "Race",
                "Asian (Not Hispanic or Latino)",
                required,
            ),
            radio(3, "race", "Race", "Decline to self-identify", required),
        ]
    };
    let optional = plan_fields(&group(false), &race_bank(), Path::new("cv.pdf"));
    assert!(optional.actions.is_empty());
    assert!(optional.missing.is_empty());
    let required = plan_fields(&group(true), &race_bank(), Path::new("cv.pdf"));
    assert!(required.actions.is_empty());
    assert_eq!(required.missing, vec!["Race"]);
    // A plain "White" option beside a separate "Hispanic or Latino" one (Greenhouse) is no
    // longer picked: in a single-choice list the two exclude each other, so "White" would
    // answer the Hispanic question too. Required: Gonza's. Optional: left alone.
    let plain = |required: bool| {
        [
            radio(0, "race", "Race", "Hispanic or Latino", required),
            radio(1, "race", "Race", "White", required),
        ]
    };
    let plan = plan_fields(&plain(true), &race_bank(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing, vec!["Race"]);
    let plan = plan_fields(&plain(false), &race_bank(), Path::new("cv.pdf"));
    assert!(plan.actions.is_empty() && plan.missing.is_empty());
}

#[test]
fn a_drop_down_option_is_the_exact_one_then_the_prefix_never_a_loose_match() {
    let list = texts(&["Chile Islands", "Chile +56", "Chile"]);
    // The exact option wins even when a longer one comes first; without it the first option
    // that starts with the answer as a whole word is the one.
    assert_eq!(best_option(&list, "Chile"), Some(2));
    assert_eq!(best_option(&list[..2], "Chile"), Some(0));
    assert_eq!(
        best_option(&texts(&["Not sure", "No, thanks"]), "No"),
        Some(1)
    );
    // No answer without words in common, and no loose match for a plain answer: "Computer
    // Engineering" is not "Computer Science and Engineering".
    assert_eq!(best_option(&texts(&["Asian", "Other"]), "White"), None);
    assert_eq!(
        best_option(
            &texts(&["Computer Science and Engineering"]),
            "Computer Engineering"
        ),
        None
    );
    // Never an option that says "(Not Hispanic or Latino)".
    let race = texts(&["White (Not Hispanic or Latino)", "White"]);
    assert_eq!(best_option(&race, "White"), Some(1));
    assert_eq!(best_option(&race[..1], "White"), None);
}

#[test]
fn a_place_matches_an_option_that_names_all_its_parts() {
    // The city list answers "Santiago" with full place names; the commas of the bank's
    // "Santiago, Chile" find nothing when typed, so only its first part is searched.
    let places = texts(&[
        "Santiago de Cali, Valle del Cauca, Colombia",
        "Santiago, Dominican Republic",
        "Santiago, Metropolitana, Chile",
    ]);
    assert_eq!(search_text("Santiago, Chile"), "Santiago");
    assert_eq!(best_option(&places, "Santiago, Chile"), Some(2));
    assert_eq!(best_option(&places[..2], "Santiago, Chile"), None);
    assert_eq!(search_text("Engineer's Degree"), "Engineer's Degree");
}

#[test]
fn a_text_input_with_alternatives_may_be_a_custom_dropdown() {
    // Greenhouse's drop-downs are inputs, not selects: the filler needs every option text to
    // try. A plain text box (and a textarea) takes the first one only.
    let bank = bank_with(vec![
        AnswerEntry::new(&["=school"], Some("Example Professional Institute"))
            .with_alternatives(&["Example PI", "Other"]),
    ]);
    let fields = [
        field(0, "input", "text", "School*", true),
        field(1, "textarea", "", "School", false),
    ];
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert_eq!(
        plan.actions[0].value,
        FillValue::Select(vec![
            "Example Professional Institute".into(),
            "Example PI".into(),
            "Other".into()
        ])
    );
    assert_eq!(
        plan.actions[1].value,
        FillValue::Text("Example Professional Institute".into())
    );
}

#[test]
fn required_checkbox_and_radio_group_are_missing_once() {
    let fields = [
        field(
            0,
            "input",
            "checkbox",
            "I agree to the privacy policy",
            true,
        ),
        radio(1, "remote", "Open to remote work?", "Yes", true),
        radio(2, "remote", "Open to remote work?", "No", true),
        field(3, "input", "checkbox", "Subscribe to the newsletter", false),
    ];
    let plan = plan_fields(&fields, &bank_with_gaps(), Path::new("cv.pdf"));
    assert_eq!(
        plan.missing,
        vec!["I agree to the privacy policy", "Open to remote work?"]
    );
    assert!(plan.actions.is_empty());
}

fn languages_bank(value: Option<&str>) -> AnswerBank {
    bank_with(vec![AnswerEntry::new(&["which languages"], value)])
}

const LANGUAGES: &str = "Which languages do you use? (select all that apply) *";

#[test]
fn a_consent_phrase_in_the_question_of_a_group_ticks_none_of_its_options() {
    // The question mentions the privacy notice, the options are channels: ticking "every
    // option because the question sounds like consent" would sign Gonza up for all of them.
    let question = "Review our privacy notice, then choose how we may reach you *";
    let fields = checkbox_group(0, question, &["Email", "SMS", "Phone"], true);
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy notice".into()],
        ..bank_with_gaps()
    };
    let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing, vec![question]);
}

#[test]
fn a_consent_box_whose_text_names_hispanic_or_latino_is_never_ticked() {
    // Found by the fuzz body: a page can caption an ethnicity statement as the privacy
    // consent, and ticking it would answer the Hispanic or Latino question, left to Gonza.
    let allowed = AnswerBank {
        consent_phrases: vec!["privacy policy".into()],
        ..bank_with_gaps()
    };
    let caption = "I agree to the privacy policy *";
    let pages = [
        // Alone in a captioned group: the caption as its question, the statement as its text.
        vec![checkbox(
            0,
            "consent",
            caption,
            "Not Hispanic or Latino",
            true,
        )],
        // The scan's form for such a box: "caption: option" as one label.
        vec![field(
            0,
            "input",
            "checkbox",
            &single_checkbox_label(caption, "I am not of Latino descent"),
            true,
        )],
    ];
    for fields in pages {
        let plan = plan_fields(&fields, &allowed, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty(), "{:?}", plan.actions);
        assert_eq!(plan.missing.len(), 1, "{:?}", plan.missing);
    }
}

#[test]
fn a_checkbox_group_ticks_every_option_the_bank_lists() {
    let fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
    let plan = plan_fields(
        &fields,
        &languages_bank(Some("Python; Rust")),
        Path::new("cv.pdf"),
    );
    assert!(plan.missing.is_empty(), "{:?}", plan.missing);
    let ticked = action_idxs(&plan);
    assert_eq!(ticked, vec![0, 1]);
    assert!(
        plan.actions
            .iter()
            .all(|action| action.value == FillValue::Check && action.label == LANGUAGES)
    );
}

#[test]
fn a_checkbox_group_takes_the_first_candidate_the_form_has() {
    // Like a drop-down: the value, then each alternative; the first one that names an option
    // wins and nothing else is ticked.
    let bank = bank_with(vec![
        AnswerEntry::new(&["how did you hear"], Some("Company careers page"))
            .with_alternatives(&["Company website", "Other"]),
    ]);
    let question = "How did you hear about us? (select all that apply) *";
    let fields = checkbox_group(0, question, &["LinkedIn", "Glassdoor", "Other"], true);
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].idx, 2);
    // None of the candidates is an option here: a required group is Gonza's.
    let fields = checkbox_group(0, question, &["LinkedIn", "Glassdoor"], true);
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.actions.is_empty());
    assert_eq!(plan.missing, vec![question]);
}

#[test]
fn a_checkbox_group_without_an_approved_answer_stays_unticked() {
    // No entry, an entry without a value and one with a blank value all mean "ask Gonza":
    // required, the question is reported once; optional, it is simply left alone.
    let banks = [
        AnswerBank::default(),
        languages_bank(None),
        languages_bank(Some("   ")),
    ];
    for bank in &banks {
        let required = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
        let plan = plan_fields(&required, bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty());
        assert_eq!(plan.missing, vec![LANGUAGES]);
        let optional = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], false);
        let plan = plan_fields(&optional, bank, Path::new("cv.pdf"));
        assert!(plan.actions.is_empty());
        assert!(plan.missing.is_empty());
    }
}

#[test]
fn an_optional_checkbox_group_is_ticked_when_the_bank_answers_it() {
    // Like an optional radio group: an explicit answer is applied, required or not.
    let fields = checkbox_group(0, "Which languages do you use?", &["Python", "Go"], false);
    let plan = plan_fields(&fields, &languages_bank(Some("Go")), Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].idx, 1);
    assert!(plan.missing.is_empty());
}

#[test]
fn a_checkbox_group_never_ticks_a_not_hispanic_option() {
    let options = [
        "White (Not Hispanic or Latino)",
        "Hispanic or Latino",
        "Decline to self-identify",
    ];
    let question = "Race or ethnicity (select all that apply) *";
    let fields = checkbox_group(0, question, &options, true);
    let bank = bank_with(vec![AnswerEntry::new(
        &["race or ethnicity"],
        Some("White"),
    )]);
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.missing, vec![question]);
}

#[test]
fn an_option_that_is_already_ticked_is_kept_and_the_others_are_ticked() {
    let mut fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
    fields[0].checked = true;
    let plan = plan_fields(
        &fields,
        &languages_bank(Some("Python; Rust")),
        Path::new("cv.pdf"),
    );
    assert_eq!(plan.kept, vec![0]);
    let ticked = action_idxs(&plan);
    assert_eq!(ticked, vec![1]);
}

#[test]
fn an_open_checkbox_group_is_left_to_gonza_once_on_its_first_option() {
    let mut fields = checkbox_group(0, LANGUAGES, &["Python", "Rust", "Go"], true);
    fields.extend(checkbox_group(
        3,
        "Which tools? (select all that apply) *",
        &["Git", "Make"],
        true,
    ));
    // The languages are answered (so they are not flagged), the tools are open.
    let bank = bank_with(vec![AnswerEntry::new(&["which languages"], Some("Go"))]);
    let plan = plan_fields(&fields, &bank, Path::new("cv.pdf"));
    assert_eq!(plan.missing, vec!["Which tools? (select all that apply) *"]);
    let left = controls_left(&fields, &plan, &[]);
    assert_eq!(left, vec![3]);
    // The mark is watched through every option of the group, so ticking any of them clears it.
    assert_eq!(flag_groups(&fields, &left), vec![vec![3, 4]]);
}

#[test]
fn a_lone_checkbox_with_a_caption_is_still_judged_as_a_consent_box() {
    // One option: not a group. The scan labels it "question: option" and the allow list
    // decides, exactly as before checkbox groups were decided as a whole.
    let alone = [checkbox(
        0,
        "ack[]",
        "Point of Data Transfer: Acknowledge",
        "Acknowledge",
        true,
    )];
    let allowed = AnswerBank {
        consent_phrases: vec!["data transfer".into()],
        ..AnswerBank::default()
    };
    let plan = plan_fields(&alone, &allowed, Path::new("cv.pdf"));
    assert_eq!(plan.actions.len(), 1);
    assert!(plan.missing.is_empty());
}
