//! Tests of what leaves the panel (criterion A9, widened): one filter on every JSON reply takes the folder
//! of the Windows user and his own addresses out of every text field but the links.

use super::support_tests::{encode, guard, json_of, request, without};
use super::*;
use crate::fixtures::panel;
use jobbot_domain::state::State;

#[test]
fn consecutive_replies_share_one_reading_of_the_profile_files() {
    let (mut p, dir) = panel();
    let profile = dir.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        profile.join("claude.toml"),
        "expected_email = \"mio@mail.example\"\n",
    )
    .unwrap();
    p.paths.profile_dir = profile.clone();
    let req = without(request("GET", "/api/snapshot", ""), TOKEN_HEADER);
    for _ in 0..3 {
        handle(&p, &guard(), &req);
        refuse_early(&p, &guard(), &req).expect("refused");
    }
    assert_eq!(p.privacy.loads(), 1, "six replies, one read of the files");
    // Edited while the panel is open: the next reply sees it.
    std::fs::write(profile.join("answers.toml"), "= broken").unwrap();
    handle(&p, &guard(), &req);
    assert_eq!(p.privacy.loads(), 2);
}

/// Every string of `value` with the path to it, leaving out the ones under a link field.
fn leaves(value: &Value, path: &str, out: &mut Vec<(String, String)>) {
    match value {
        Value::String(text) => out.push((path.to_string(), text.clone())),
        Value::Array(items) => {
            for (n, item) in items.iter().enumerate() {
                leaves(item, &format!("{path}[{n}]"), out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if ["url", "apply_url", "link"].contains(&key.as_str()) && item.is_string() {
                    continue;
                }
                leaves(item, &format!("{path}.{key}"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn the_folder_of_the_user_and_his_own_addresses_never_leave_the_panel_in_any_text_field() {
    const FOLDER: &str = r"C:\Users\ana\AppData\Local\jobbot\x.json";
    const MINE: &str = "mio@mail.example";
    const ALSO_MINE: &str = "yo.mismo@mail.example";
    let now = *jobbot_testkit::NOW;
    // The profile says which addresses are his: `expected_email` and the e-mail the answer bank
    // fills into forms.
    let (mut p, dir) = panel();
    let profile = dir.path().join("profile");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(
        profile.join("claude.toml"),
        format!("expected_email = \"{MINE}\"\n"),
    )
    .unwrap();
    std::fs::write(
        profile.join("answers.toml"),
        format!("[[entry]]\npatterns = [\"email\"]\nvalue = \"{ALSO_MINE}\"\n"),
    )
    .unwrap();
    p.paths.profile_dir = profile;
    let tainted = |what: &str| format!("{what} {FOLDER} {MINE} / {ALSO_MINE}");

    let job = jobbot_testkit::make_job_with(|j| {
        j.external_id = "leak".into();
        j.company = tainted("Empresa");
        j.title = tainted("Cargo");
        j.location = tainted("Lugar");
        j.description = tainted("Descripción");
        j.url = format!("https://empresa.example/jobs/1?c={MINE}");
        j.apply_url = format!("https://empresa.example/apply?c={MINE}");
    });
    p.tracker
        .add_job(&job, 80.0, &std::collections::BTreeMap::new(), now)
        .unwrap();
    let key = job.key();
    p.tracker
        .start_application(&key, jobbot_domain::origin::Origin::Cold, now)
        .unwrap();
    p.tracker
        .transition(&key, State::Prepared, None, now)
        .unwrap();
    p.tracker
        .transition(&key, State::NeedsGonza, Some(&tainted("page_error:")), now)
        .unwrap();
    std::fs::create_dir_all(p.paths.data_dir.join("dossiers")).unwrap();
    std::fs::write(
        jobbot_app::learn::dossier::dossier_path(&p.paths.data_dir.join("dossiers"), &key),
        tainted("# Dossier"),
    )
    .unwrap();
    p.tracker
        .add_task(
            &jobbot_app::ports::store::NewTask {
                job_key: Some(key.clone()),
                kind: "action_required".into(),
                text: tainted("Tarea"),
                due_at: None,
                source_message_id: None,
                link: Some(format!("https://empresa.example/t?c={MINE}")),
            },
            now,
        )
        .unwrap();
    let mail = |id: &str, from: &str| jobbot_app::ports::store::NewEmail {
        message_id: id.into(),
        thread_id: format!("th-{id}"),
        from_addr: from.into(),
        subject: tainted("Asunto"),
        received_at: now,
        job_key: Some(key.clone()),
        kind: jobbot_app::ports::store::EmailKind::Recruiter,
        summary: tainted("Resumen"),
    };
    p.tracker
        .add_email(&mail("m1", "Zoe <zoe@acme.example>"), None, now)
        .unwrap();
    p.tracker
        .add_email(&mail("m2", &format!("Yo <{MINE}>")), None, now)
        .unwrap();

    let snapshot = json_of(&handle(&p, &guard(), &request("GET", "/api/snapshot", "")));
    let detail = json_of(&handle(
        &p,
        &guard(),
        &request("GET", &format!("/api/job?key={}", encode(&key)), ""),
    ));

    let listing = json_of(&handle(&p, &guard(), &request("GET", "/api/jobs", "")));
    for (name, reply, least) in [
        ("snapshot", &snapshot, 20),
        ("job", &detail, 20),
        ("jobs", &listing, 5),
    ] {
        let mut all = Vec::new();
        leaves(reply, "", &mut all);
        assert!(
            all.len() > least,
            "{name}: the walk saw {} strings",
            all.len()
        );
        for (path, field_text) in &all {
            for secret in [MINE, ALSO_MINE, r"C:\Users\ana", "AppData"] {
                assert!(
                    !field_text.contains(secret),
                    "{name}{path} carries {secret:?}: {field_text:?}"
                );
            }
        }
    }
    // They were seeded in every field that has text, and each one came out masked, not cut.
    let masked = "m***@mail.example";
    assert!(
        listing["cards"][0]["company"]
            .as_str()
            .unwrap()
            .contains(masked),
        "the explorer is filtered like the snapshot"
    );
    let card = &snapshot["needs_gonza"]["cards"][0];
    let events = detail["events"].as_array().unwrap();
    for (what, got) in [
        ("task", &snapshot["today"]["tasks"][0]["text"]),
        ("mail subject", &snapshot["today"]["emails"][0]["subject"]),
        ("mail summary", &snapshot["today"]["emails"][0]["summary"]),
        ("card company", &card["company"]),
        ("card title", &card["title"]),
        ("card reason", &card["reason"]),
        ("card note", &card["note"]),
        ("move company", &snapshot["activity"]["moves"][0]["company"]),
        ("detail location", &detail["location"]),
        ("detail description", &detail["description"]),
        ("detail dossier", &detail["dossier"]),
        ("detail event note", &events[events.len() - 1]["note"]),
    ] {
        let got = got
            .as_str()
            .unwrap_or_else(|| panic!("{what} is not text: {got}"));
        assert!(got.contains(masked), "{what}: {got}");
        assert!(got.contains("%LOCALAPPDATA%"), "{what}: {got}");
    }
    // A company's address is content the page needs; a link still opens as it was.
    let senders: Vec<&str> = snapshot["today"]["emails"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["from"].as_str().unwrap())
        .collect();
    assert!(senders.contains(&"Zoe <zoe@acme.example>"));
    assert!(senders.contains(&"Yo <m***@mail.example>"));
    assert_eq!(
        card["url"],
        format!("https://empresa.example/jobs/1?c={MINE}")
    );
    assert_eq!(
        detail["apply_url"],
        format!("https://empresa.example/apply?c={MINE}")
    );
    assert_eq!(
        snapshot["today"]["tasks"][0]["link"],
        format!("https://empresa.example/t?c={MINE}")
    );
}

#[test]
fn an_error_that_names_a_folder_of_the_user_reaches_the_page_without_it() {
    let user = UserPaths::new(Some(r"C:\Users\ana"), None, None);
    let direct = redact_reply(
        &user,
        Reply::message(
            500,
            r"Error interno: no pude abrir C:\Users\ana\data\jobbot.db",
        ),
    );
    assert_eq!(
        json_of(&direct)["message"],
        r"Error interno: no pude abrir %USERPROFILE%\data\jobbot.db"
    );
    // Only JSON is touched: a font, a screenshot or a document keeps its bytes.
    let page = Reply {
        status: 200,
        content_type: "text/markdown; charset=utf-8",
        headers: Vec::new(),
        body: br"ruta C:\Users\ana\x".to_vec(),
    };
    assert_eq!(redact_reply(&user, page).body, br"ruta C:\Users\ana\x");
}
