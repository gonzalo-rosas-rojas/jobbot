//! The fuzz targets' bodies (`jobbot_testkit::fuzz`) under proptest, which runs on Windows where
//! cargo-fuzz does not: arbitrary bytes, bytes shaped like each input, and the hostile corpus with
//! bytes flipped.

use proptest::collection::vec;
use proptest::prelude::*;

use jobbot_testkit::NOW;
use jobbot_testkit::fuzz::{address, email_import, form_plan};
use jobbot_testkit::hostile::{LINKS, emails};

/// Records of a page scan as `fuzz::fields` reads them: five picking bytes and some free text.
fn scan_bytes() -> impl Strategy<Value = Vec<u8>> {
    vec((any::<[u8; 5]>(), "\\PC{0,24}"), 0..24).prop_map(|records| {
        let mut bytes = Vec::new();
        for (picks, text) in records {
            bytes.extend_from_slice(&picks);
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(0xFF);
        }
        bytes
    })
}

/// The hostile e-mails as an import file, with up to eight bytes changed.
fn hostile_file() -> impl Strategy<Value = Vec<u8>> {
    let file = serde_json::to_vec(
        &emails(*NOW)
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let len = file.len();
    vec((0..len, any::<u8>()), 0..8).prop_map(move |flips| {
        let mut bytes = file.clone();
        for (at, byte) in flips {
            bytes[at] = byte;
        }
        bytes
    })
}

fn link_bytes() -> impl Strategy<Value = Vec<u8>> {
    let links: Vec<Vec<u8>> = LINKS
        .iter()
        .map(|(_, link)| link.as_bytes().to_vec())
        .collect();
    prop_oneof![
        proptest::sample::select(links),
        "(https?|file|data|javascript)://[a-z0-9.@:%\\[\\]-]{0,24}(/[ -~]{0,16})?"
            .prop_map(String::into_bytes),
        vec(any::<u8>(), 0..64),
    ]
}

proptest! {
    #[test]
    fn the_form_planner_keeps_its_rules_on_any_scan(
        data in prop_oneof![scan_bytes(), vec(any::<u8>(), 0..512)]
    ) {
        form_plan(&data);
    }

    #[test]
    fn the_email_import_keeps_its_rules_on_any_file(
        data in prop_oneof![hostile_file(), vec(any::<u8>(), 0..512)]
    ) {
        email_import(&data);
    }

    #[test]
    fn the_address_check_keeps_its_rules_on_any_link(data in link_bytes()) {
        address(&data);
    }
}

#[test]
fn the_hostile_file_imports_whole_before_any_byte_changes() {
    let file = serde_json::to_vec(
        &emails(*NOW)
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    email_import(&file);
}
