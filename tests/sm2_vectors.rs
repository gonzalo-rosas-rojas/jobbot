//! The SM-2 rule must give the same answers as the artifact panel (JavaScript). Both sides read
//! `tests/fixtures/sm2_vectors.json`, so a change to the rule in one place breaks a test in the other.

use std::path::Path;

use jobbot::prep::sm2;
use serde::Deserialize;

#[derive(Deserialize)]
struct Expected {
    ease: f64,
    interval: i64,
    reps: i64,
}

#[derive(Deserialize)]
struct Vector {
    name: String,
    ease: f64,
    interval: i64,
    reps: i64,
    grade: u8,
    expected: Expected,
}

#[derive(Deserialize)]
struct Vectors {
    vectors: Vec<Vector>,
}

#[test]
fn sm2_matches_shared_vectors() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sm2_vectors.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let file: Vectors = serde_json::from_str(&text).unwrap();
    assert!(
        file.vectors.len() >= 15,
        "the shared file should cover every grade and the edge cases"
    );
    for v in &file.vectors {
        let (ease, interval, reps) = sm2(v.ease, v.interval, v.reps, v.grade);
        assert!(
            (ease - v.expected.ease).abs() < 1e-9,
            "{}: ease {ease} != {}",
            v.name,
            v.expected.ease
        );
        assert_eq!(interval, v.expected.interval, "{}: interval", v.name);
        assert_eq!(reps, v.expected.reps, "{}: reps", v.name);
    }
}
