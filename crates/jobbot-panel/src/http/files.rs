//! The documents and screenshots served from the user's folders. A request names an application and a
//! kind, never a path, and the file has to sit under its own folder of `data/`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::Reply;
use crate::Panel;
use jobbot_app::learn::dossier::dossier_path;
use jobbot_domain::handoff::sheet_path;

const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;
/// The most a CV, dossier or hand-over sheet served by `/api/file` may weigh.
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

/// The three documents an application can have on disk.
#[derive(Clone, Copy)]
pub(super) enum DocumentKind {
    Cv,
    Dossier,
    Handoff,
}

/// How one kind of document is found and served (see `DocumentKind::rules`).
struct DocumentRules {
    folder: &'static str,
    extension: &'static str,
    content_type: &'static str,
    missing: &'static str,
}

impl DocumentKind {
    pub(super) fn parse(text: &str) -> Option<DocumentKind> {
        match text {
            "cv" => Some(DocumentKind::Cv),
            "dossier" => Some(DocumentKind::Dossier),
            "handoff" => Some(DocumentKind::Handoff),
            _ => None,
        }
    }

    /// The folder under `data/` it lives in, the extension it must have, what it is served as
    /// and what is said when it is not there.
    fn rules(self) -> DocumentRules {
        match self {
            DocumentKind::Cv => DocumentRules {
                folder: "cv",
                extension: "pdf",
                content_type: "application/pdf",
                missing: "Sin CV",
            },
            DocumentKind::Dossier => DocumentRules {
                folder: "dossiers",
                extension: "md",
                content_type: "text/markdown; charset=utf-8",
                missing: "Sin dossier",
            },
            DocumentKind::Handoff => DocumentRules {
                folder: "handoff",
                extension: "md",
                content_type: "text/markdown; charset=utf-8",
                missing: "Sin hoja de traspaso",
            },
        }
    }
}

/// `stored` as the file it really is (`canonicalize`d, so a `..` or a link is followed), when it
/// sits under `root`, also `canonicalize`d. `None` when either cannot be resolved or the file is
/// outside the folder. The one guard of what the panel serves from disk: `document` and
/// `screenshot` both go through it.
fn file_under(root: &Path, stored: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let file = std::fs::canonicalize(stored).ok()?;
    file.starts_with(&root).then_some(file)
}

/// The CV, dossier or hand-over sheet of an application, as the file it is. It is the same guard as
/// `screenshot`, because this is not a file server either: the request names an application and a
/// kind and never a path; the file is the one the engine keeps for that application (the CV from
/// the database, the others from the key), it must sit under its own folder of `data/` once both
/// are `canonicalize`d (so a `..` or a link cannot leave it), have the extension of its kind, be a
/// plain file and weigh at most `MAX_DOCUMENT_BYTES`. Anything else is a 404 that does not say
/// which of those failed.
pub(super) fn document(panel: &Panel, key: &str, kind: DocumentKind) -> Result<Reply> {
    let DocumentRules {
        folder,
        extension,
        content_type,
        missing,
    } = kind.rules();
    let none = || Ok(Reply::message(404, missing));
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(Reply::message(404, "No existe esa postulación"));
    };
    let root = panel.paths.data_dir.join(folder);
    let stored = match kind {
        DocumentKind::Cv => row.cv_path.map(PathBuf::from),
        DocumentKind::Dossier => Some(dossier_path(&root, key)),
        DocumentKind::Handoff => Some(sheet_path(&root, key)),
    };
    let Some(stored) = stored else {
        return none();
    };
    let Some(file) = file_under(&root, &stored) else {
        return none();
    };
    if !file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return none();
    }
    match std::fs::metadata(&file) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_DOCUMENT_BYTES => {}
        _ => return none(),
    }
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| {
            n.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        .map_or_else(|| format!("{folder}.{extension}"), str::to_string);
    let mut headers = vec![(
        "Content-Disposition",
        format!("inline; filename=\"{name}\""),
    )];
    if content_type.starts_with("text/") {
        // Text shown by the browser must not be able to run or load anything.
        headers.push((
            "Content-Security-Policy",
            "default-src 'none'; sandbox".to_string(),
        ));
    }
    Ok(Reply {
        status: 200,
        content_type,
        headers,
        body: std::fs::read(&file)?,
    })
}

/// The dry-run screenshot of an application. Only files under `data/screenshots` are served: this
/// endpoint shows screenshots, it is not a file server, whatever path the database holds.
pub(super) fn screenshot(panel: &Panel, key: &str) -> Result<Reply> {
    let none = || Ok(Reply::message(404, "Sin captura"));
    let Some(row) = panel.tracker.get_application(key)? else {
        return Ok(Reply::message(404, "No existe esa postulación"));
    };
    let Some(stored) = row.screenshot_path else {
        return none();
    };
    let Some(file) = file_under(
        &panel.paths.data_dir.join("screenshots"),
        Path::new(&stored),
    ) else {
        return none();
    };
    let content_type = match file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => return none(),
    };
    if std::fs::metadata(&file)?.len() > MAX_SCREENSHOT_BYTES {
        return none();
    }
    Ok(Reply {
        status: 200,
        content_type,
        headers: Vec::new(),
        body: std::fs::read(&file)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{add, panel};
    use crate::http::handle;
    use crate::http::support_tests::{api_get, encode, guard, json_of, request};

    #[test]
    fn a_screenshot_is_served_only_from_the_screenshots_folder_and_a_failure_is_a_500() {
        let (p, dir) = panel();
        let shots = dir.path().join("data").join("screenshots");
        std::fs::create_dir_all(&shots).unwrap();
        let get = |key: &str| {
            handle(
                &p,
                &guard(),
                &request("GET", &format!("/api/shot?key={}", encode(key)), ""),
            )
        };

        let inside = add(&p, "in", 80.0, &[]);
        let file = shots.join("a.png");
        std::fs::write(&file, b"PNGDATA").unwrap();
        p.tracker.set_screenshot(&inside, &file).unwrap();
        let ok = get(&inside);
        assert_eq!((ok.status, ok.content_type), (200, "image/png"));
        assert_eq!(ok.body, b"PNGDATA");

        let outside = add(&p, "out", 80.0, &[]);
        // Next to the screenshots folder, not in it: `shots/../secret.png` is this same file.
        let secret = dir.path().join("data").join("secret.png");
        std::fs::write(&secret, b"SECRET").unwrap();
        p.tracker.set_screenshot(&outside, &secret).unwrap();
        assert_eq!(get(&outside).status, 404);

        let dotdot = add(&p, "dots", 80.0, &[]);
        p.tracker
            .set_screenshot(&dotdot, &shots.join("..").join("secret.png"))
            .unwrap();
        assert_eq!(get(&dotdot).status, 404, "a .. path that leaves the folder");

        let none = add(&p, "none", 80.0, &[]);
        assert_eq!(get(&none).status, 404);
        assert_eq!(get("nope:1").status, 404);

        let broken = add(&p, "dir", 80.0, &[]);
        let folder = shots.join("x.png");
        std::fs::create_dir_all(&folder).unwrap();
        p.tracker.set_screenshot(&broken, &folder).unwrap();
        let failed = get(&broken);
        assert_eq!(failed.status, 500);
        assert!(
            json_of(&failed)["message"]
                .as_str()
                .unwrap()
                .starts_with("Error interno")
        );
        assert_eq!(
            get(&inside).status,
            200,
            "the next request is served normally"
        );
    }

    /// `GET /api/file` for `key` and `kind`.
    fn file_of(p: &Panel, key: &str, kind: &str) -> Reply {
        api_get(p, &format!("/api/file?key={}&kind={kind}", encode(key)))
    }

    #[test]
    fn a_document_is_served_by_key_and_kind_with_its_own_type_and_untouched() {
        let (p, dir) = panel();
        let data = dir.path().join("data");
        let key = add(&p, "1", 80.0, &[]);
        let other = add(&p, "2", 80.0, &[]);
        // The CV is where the engine put it, named by the key; the dossier and the hand-over
        // sheet are found from the key alone.
        let cv = data
            .join("cv")
            .join(format!("{}.pdf", jobbot_domain::text::safe_key(&key)));
        let dossier = jobbot_app::learn::dossier::dossier_path(&data.join("dossiers"), &key);
        let sheet = jobbot_domain::handoff::sheet_path(&data.join("handoff"), &key);
        for path in [&cv, &dossier, &sheet] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        std::fs::write(&cv, b"%PDF-1.4 fake").unwrap();
        std::fs::write(
            &dossier,
            "# Dossier\nme@mail.example en C:\\Users\\ana\\x\n",
        )
        .unwrap();
        std::fs::write(&sheet, "# Hoja\nResponde: si\n").unwrap();
        p.tracker.set_cv_path(&key, &cv).unwrap();

        let pdf = file_of(&p, &key, "cv");
        assert_eq!((pdf.status, pdf.content_type), (200, "application/pdf"));
        assert_eq!(pdf.body, b"%PDF-1.4 fake");
        assert_eq!(
            pdf.header("content-disposition"),
            Some(
                format!(
                    "inline; filename=\"{}.pdf\"",
                    jobbot_domain::text::safe_key(&key)
                )
                .as_str()
            )
        );
        for (kind, text) in [("dossier", "# Dossier"), ("handoff", "# Hoja")] {
            let reply = file_of(&p, &key, kind);
            assert_eq!(reply.status, 200, "{kind}");
            assert_eq!(reply.content_type, "text/markdown; charset=utf-8", "{kind}");
            // A document that is shown as text must not be able to run anything.
            assert!(
                reply
                    .header("content-security-policy")
                    .is_some_and(|csp| csp.contains("sandbox")),
                "{kind}"
            );
            assert_eq!(reply.header("cache-control"), Some("no-store"));
            assert!(String::from_utf8(reply.body).unwrap().starts_with(text));
        }
        // It is the document itself, not a view of it: the filter that keeps his folder and his
        // address out of the snapshot does not rewrite his own file.
        let raw = String::from_utf8(file_of(&p, &key, "dossier").body).unwrap();
        assert!(raw.contains("me@mail.example") && raw.contains("C:\\Users\\ana\\x"));
        // Another application has none of these.
        for kind in ["cv", "dossier", "handoff"] {
            assert_eq!(file_of(&p, &other, kind).status, 404, "{kind}");
        }
    }

    #[test]
    fn a_document_is_never_served_from_outside_its_folder_nor_as_something_else() {
        let (p, dir) = panel();
        let data = dir.path().join("data");
        std::fs::create_dir_all(data.join("cv")).unwrap();
        let secret = data.join("secret.pdf");
        std::fs::write(&secret, b"SECRET").unwrap();
        let outside_the_data = dir.path().join("elsewhere.pdf");
        std::fs::write(&outside_the_data, b"ELSEWHERE").unwrap();

        let set = |id: &str, path: &std::path::Path| {
            let key = add(&p, id, 80.0, &[]);
            p.tracker.set_cv_path(&key, path).unwrap();
            key
        };
        // A CV recorded by hand with `jobbot record --cv <anywhere>` is not in the CV folder.
        let manual = set("manual", &outside_the_data);
        // Next to the folder, not in it: `cv/../secret.pdf` is this same file.
        let sibling = set("sibling", &secret);
        let dotdot = set("dotdot", &data.join("cv").join("..").join("secret.pdf"));
        // In the folder but not a PDF.
        let notes_file = data.join("cv").join("notes.txt");
        std::fs::write(&notes_file, b"notes").unwrap();
        let not_pdf = set("text", &notes_file);
        // A folder where a file should be, and a name that is gone.
        let folder = data.join("cv").join("folder.pdf");
        std::fs::create_dir_all(&folder).unwrap();
        let is_dir = set("dir", &folder);
        let gone = set("gone", &data.join("cv").join("gone.pdf"));
        // Too big to serve.
        let huge = data.join("cv").join("huge.pdf");
        std::fs::write(&huge, vec![0u8; (MAX_DOCUMENT_BYTES + 1) as usize]).unwrap();
        let too_big = set("huge", &huge);
        // And one that is fine, so these 404s are the guard and not a broken test.
        let fine_path = data.join("cv").join("fine.pdf");
        std::fs::write(&fine_path, b"%PDF").unwrap();
        let fine = set("fine", &fine_path);

        for (what, key) in [
            ("outside data", &manual),
            ("sibling folder", &sibling),
            ("dotdot", &dotdot),
            ("not a pdf", &not_pdf),
            ("a folder", &is_dir),
            ("gone", &gone),
            ("too big", &too_big),
        ] {
            let reply = file_of(&p, key, "cv");
            assert_eq!(reply.status, 404, "{what}");
            assert!(
                !String::from_utf8_lossy(&reply.body).contains("SECRET"),
                "{what}"
            );
        }
        assert_eq!(file_of(&p, &fine, "cv").status, 200);
        // The key names an application, never a path.
        for key in ["../../secret", "..\\..\\x", "nope:1"] {
            assert_eq!(file_of(&p, key, "dossier").status, 404, "{key}");
        }
    }

    #[test]
    fn a_document_request_that_makes_no_sense_is_a_400() {
        let (p, _d) = panel();
        let key = add(&p, "1", 80.0, &[]);
        let get = |target: &str| handle(&p, &guard(), &request("GET", target, ""));
        for target in [
            "/api/file".to_string(),
            format!("/api/file?key={}", encode(&key)),
            "/api/file?kind=cv".to_string(),
            format!("/api/file?key={}&kind=", encode(&key)),
            format!("/api/file?key={}&kind=shot", encode(&key)),
            format!("/api/file?key={}&kind=..%2F..%2Fx", encode(&key)),
        ] {
            let reply = get(&target);
            assert_eq!(reply.status, 400, "{target}");
            assert_eq!(json_of(&reply)["ok"], false, "{target}");
        }
    }
}
