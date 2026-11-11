//! The run record and the status lines over the real SQLite store: what `run-daily` writes is what
//! `jobbot status` reads back.

use chrono::TimeDelta;
use serde_json::json;

use jobbot_app::reporting::status::{RunEnd, StepLog, describe_last_run, record_run};
use jobbot_testkit::{NOW, Tracker};

#[test]
fn before_any_run_status_says_there_is_none() {
    let tracker = Tracker::in_memory().unwrap();
    let line = describe_last_run(&tracker, *NOW).unwrap();
    assert!(
        line.starts_with("Última corrida: ninguna todavía"),
        "{line}"
    );
}

#[test]
fn a_recorded_run_is_what_status_reads_back() {
    let tracker = Tracker::in_memory().unwrap();
    let mut log = StepLog::default();
    log.ran("discover", json!({"new": 1}));
    log.ran("prepare", json!({"prepared": 1}));
    record_run(&tracker, *NOW, &RunEnd::WindowClosed, &mut log).unwrap();

    let line = describe_last_run(&tracker, *NOW + TimeDelta::hours(2)).unwrap();
    assert!(
        line.contains("(hora de Santiago, hace 2 h): se cerró la ventana de ejecución"),
        "{line}"
    );
    assert!(
        line.ends_with(
            "descubrir: 1 nuevas · preparar: 1 preparadas · enviar: omitido (ventana cerrada) · \
             dossiers: omitido (ventana cerrada)"
        ),
        "{line}"
    );
}
