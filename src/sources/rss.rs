//! A small reader for RSS 2.0 feeds, shared by the feed sources (We Work Remotely, RemoteYeah).
//!
//! It reads only what job feeds use: the `<item>` blocks and the text of the elements inside one.
//! CDATA is unwrapped and entities are decoded, so a field arrives as plain text; a description
//! arrives as the HTML the feed meant, whether the feed escaped it or wrapped it in CDATA.

use std::sync::LazyLock;

use anyhow::bail;
use regex::Regex;

use crate::text::decode_entities;

static ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<item(?:\s[^>]*)?>(.*?)</item>").expect("ITEM is a valid regex")
});

/// One `<item>` of a feed.
pub struct Item<'a> {
    body: &'a str,
}

impl Item<'_> {
    /// The text of the first `<tag>` in the item, trimmed; `""` when the item has no such element.
    /// A namespaced name (`dc:creator`) is matched as written.
    pub fn text(&self, tag: &str) -> String {
        element(self.body, tag).map(unwrap_text).unwrap_or_default()
    }
}

/// Every `<item>` of the feed, in order. A body that is not an RSS feed at all (an HTML block
/// page) is an error; a feed with no items is not.
pub fn items(xml: &str) -> anyhow::Result<Vec<Item<'_>>> {
    if !xml.contains("<rss") && !xml.contains("<channel") {
        let head: String = xml.chars().take(40).collect();
        bail!("response is not an RSS feed (starts with {head:?})");
    }
    Ok(ITEM
        .captures_iter(xml)
        .filter_map(|captures| captures.get(1))
        .map(|body| Item {
            body: body.as_str(),
        })
        .collect())
}

/// What lies between `<tag ...>` and `</tag>`, as written. `<tag/>` is the empty element. A longer
/// name that starts with `tag` (`<linkage>` for `link`) is not the element.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(found) = xml[from..].find(&open) {
        let name_end = from + found + open.len();
        from = name_end;
        let rest = &xml[name_end..];
        if !rest.starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace()) {
            continue;
        }
        let tag_end = name_end + rest.find('>')?;
        if xml[..tag_end].ends_with('/') {
            return Some("");
        }
        let content = &xml[tag_end + 1..];
        return Some(&content[..content.find(&format!("</{tag}>"))?]);
    }
    None
}

/// CDATA is kept as written (it holds HTML or plain text, never entities); anything else has its
/// entities decoded.
fn unwrap_text(raw: &str) -> String {
    let raw = raw.trim();
    match raw
        .strip_prefix("<![CDATA[")
        .and_then(|rest| rest.strip_suffix("]]>"))
    {
        Some(cdata) => cdata.trim().to_string(),
        None => decode_entities(raw).trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>Channel title</title>
    <link>https://example.test/feed</link>
    <item>
      <title>Stripe: Abuse Research &amp; Engineer</title>
      <region>Anywhere in the World</region>
      <country></country>
      <state/>
      <guid isPermaLink="false">stripe-abuse-1</guid>
      <linkage>not the link</linkage>
      <link>https://example.test/jobs/stripe-abuse-1</link>
      <description>&lt;p&gt;Build &lt;b&gt;things&lt;/b&gt;.&lt;/p&gt;</description>
      <dc:creator>Ann</dc:creator>
    </item>
    <item>
      <title><![CDATA[Role <with> markup & more]]></title>
      <description><![CDATA[<ul><li>Locations: Chile (Remote)</li></ul>]]></description>
    </item>
  </channel>
</rss>"#;

    #[test]
    fn reads_every_item_and_ignores_the_channel_header() {
        let items = items(FEED).unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text("title"), "Stripe: Abuse Research & Engineer");
        assert_eq!(
            items[0].text("link"),
            "https://example.test/jobs/stripe-abuse-1"
        );
    }

    #[test]
    fn text_unescapes_entities_and_unwraps_cdata() {
        let items = items(FEED).unwrap();

        assert_eq!(
            items[0].text("description"),
            "<p>Build <b>things</b>.</p>",
            "an escaped description arrives as the HTML it carries"
        );
        assert_eq!(items[1].text("title"), "Role <with> markup & more");
        assert_eq!(
            items[1].text("description"),
            "<ul><li>Locations: Chile (Remote)</li></ul>"
        );
    }

    #[test]
    fn text_matches_whole_names_attributes_and_namespaces() {
        let items = items(FEED).unwrap();

        assert_eq!(items[0].text("guid"), "stripe-abuse-1");
        assert_eq!(items[0].text("dc:creator"), "Ann");
        assert_eq!(items[0].text("region"), "Anywhere in the World");
        // An item never reads the channel's own `<link>`.
        assert_eq!(items[1].text("link"), "");
    }

    #[test]
    fn a_missing_empty_or_self_closing_element_is_empty_text() {
        let items = items(FEED).unwrap();

        assert_eq!(items[0].text("country"), "");
        assert_eq!(items[0].text("state"), "");
        assert_eq!(items[0].text("expires_at"), "");
    }

    #[test]
    fn a_body_that_is_not_a_feed_is_an_error_and_an_empty_feed_is_not() {
        let error = items("<html><body>blocked</body></html>").err().unwrap();
        assert!(error.to_string().contains("not an RSS feed"), "{error}");
        let empty = items("<rss version=\"2.0\"><channel><title>x</title></channel></rss>");
        assert!(empty.unwrap().is_empty());
    }
}
