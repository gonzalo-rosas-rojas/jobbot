//! The covers of the published repository show only pictures that are in it, each with a description.
//!
//! `docs/public/README.md` and `README.es.md` become `README.md` and `README.es.md` at the root of
//! the public copy (`xtask/src/export.rs`), so the paths they write are read from the repository
//! root, and this test reads each cover where the repository it runs in keeps it. This test finds every image they show and fails, naming the file and the path, when the
//! picture is not in the repository or has no alternative text.
//!
//! The rules are functions over text, and their own tests feed them examples, so a rule cannot
//! quietly stop biting.

use std::path::Path;

use regex::Regex;

/// The covers: where the repository keeps each, and where it sits in the public copy, which has no
/// `docs/public/` and holds each as the README of its root.
const COVERS: [(&str, &str); 2] = [
    ("docs/public/README.md", "README.md"),
    ("docs/public/README.es.md", "README.es.md"),
];

/// One picture a cover shows: its alternative text and its address.
#[derive(Debug, PartialEq)]
struct Picture {
    alt: String,
    target: String,
}

/// The pictures in `markdown`: `![alt](target)` (with an optional title) and `<img ... src="...">`.
/// An address that is a URL is not a file of the repository and is left out.
fn pictures(markdown: &str) -> Vec<Picture> {
    let inline = Regex::new(r#"!\[([^\]]*)\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)"#).unwrap();
    let tag = Regex::new(r#"(?i)<img\b[^>]*>"#).unwrap();
    let src = Regex::new(r#"(?i)\bsrc\s*=\s*["']([^"']+)["']"#).unwrap();
    let alt = Regex::new(r#"(?i)\balt\s*=\s*["']([^"']*)["']"#).unwrap();
    let mut found: Vec<Picture> = inline
        .captures_iter(markdown)
        .map(|caps| Picture {
            alt: caps[1].trim().to_string(),
            target: caps[2].to_string(),
        })
        .collect();
    for element in tag.find_iter(markdown) {
        let element = element.as_str();
        if let Some(src) = src.captures(element) {
            found.push(Picture {
                alt: alt
                    .captures(element)
                    .map_or(String::new(), |caps| caps[1].trim().to_string()),
                target: src[1].to_string(),
            });
        }
    }
    found.retain(|picture| !picture.target.contains("://") && !picture.target.starts_with("data:"));
    found
}

/// Everything wrong with the pictures of one cover. `exists` says whether a path is in the repository.
fn problems(cover: &str, markdown: &str, exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    for picture in pictures(markdown) {
        let path = picture.target.split(['#', '?']).next().unwrap_or_default();
        if !exists(path) {
            found.push(format!(
                "{cover} shows {path}, which is not in the repository"
            ));
        }
        if picture.alt.is_empty() {
            found.push(format!("{cover} shows {path} with no alternative text"));
        }
    }
    found
}

#[test]
fn the_covers_show_only_pictures_that_exist_and_describe_them() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exists = |path: &str| root.join(path).is_file();
    let mut found = Vec::new();
    let mut shown = 0;
    for (kept, published) in COVERS {
        // The repository's own root README is the owner's manual, never a cover: it is read only
        // in a public copy, where `docs/public/` is gone.
        let cover = if root.join(kept).is_file() {
            kept
        } else {
            published
        };
        let text = std::fs::read_to_string(root.join(cover))
            .unwrap_or_else(|error| panic!("{cover}: {error}"))
            .replace("\r\n", "\n");
        shown += pictures(&text).len();
        found.extend(problems(cover, &text, &exists));
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    assert!(shown > 0, "the covers show no picture at all");
}

#[cfg(test)]
mod rules {
    use super::*;

    const COVER: &str = "# Title\n\n![The panel.](docs/img/a.jpg)\n\n\
        Text with a [link](docs/b.md) and a URL image ![x](https://example.com/x.png).\n\n\
        ![With a title](docs/img/b.jpg \"A title\")\n\n\
        <img src=\"docs/img/c.jpg\" alt=\"Third\" width=\"300\">\n\n\
        ![](docs/img/d.jpg)\n";

    #[test]
    fn every_kind_of_picture_is_found_and_urls_are_not_files() {
        let found: Vec<(String, String)> = pictures(COVER)
            .into_iter()
            .map(|picture| (picture.alt, picture.target))
            .collect();
        assert_eq!(
            found,
            [
                ("The panel.", "docs/img/a.jpg"),
                ("With a title", "docs/img/b.jpg"),
                ("", "docs/img/d.jpg"),
                ("Third", "docs/img/c.jpg"),
            ]
            .map(|(alt, target)| (alt.to_string(), target.to_string()))
        );
    }

    #[test]
    fn a_missing_file_and_a_missing_description_are_named() {
        let present = |path: &str| path != "docs/img/b.jpg";
        assert_eq!(
            problems("README.md", COVER, &present),
            [
                "README.md shows docs/img/b.jpg, which is not in the repository",
                "README.md shows docs/img/d.jpg with no alternative text",
            ]
        );
        assert!(problems("README.md", COVER, &|_: &str| true).len() == 1);
    }
}
