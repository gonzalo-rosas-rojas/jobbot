# src/panel/assets: what the binary serves besides the page

Every file here is embedded with `include_bytes!` through the `ASSETS` table in `../http.rs` and served at
`GET /a/<launch-id>/<name>`, where `<launch-id>` is a random hex value chosen once per start (like the token, but
it is not a secret: it only makes every start a new cache key, so the replies may say `max-age=31536000, immutable`).
The page gets the prefix the way it gets the nonce: `__ASSETS__` in `page.html` becomes `/a/<launch-id>`, so a
stylesheet writes `url("__ASSETS__/inter-400.woff2")` (inside an `@font-face`) and a script `__ASSETS__ + "/inter-400.woff2"`.

The route is not a file server: the name is compared exactly against the table (no `..`, no sub-paths, no query),
the MIME type is the one written in the table, never guessed, and anything else is a 404. It sits under the same
`Host` guard as `/` and needs no token, because the page loads it before any script runs.

## The rule for a new asset

1. Add the file here and a row to `ASSETS` with its name, its explicit MIME type and `include_bytes!`.
2. Add a row to `THIRD_PARTY_NOTICES.md` (repo root) with the origin, the licence (`own` for something drawn here,
   SPDX id or `LicenseRef-...` otherwise), the URL it came from and its sha256 (`sha256sum <file>`; the hash goes last,
   in backticks). No row, no asset: a Rust test (`every_asset_has_a_row_...` in `http.rs`) fails when an `ASSETS` entry
   has no row, when the row does not name its MIME type or does not end in a 64-digit hash, and when a row for an
   image or a font has no entry in the table.
3. A font (`.woff2`, `.ttf`, `.otf`) goes through `ots-sanitize` first (the same sanitiser the browsers use) and only
   enters if it passes; subset it with `pyftsubset --flavor=woff2` when the whole file is not needed. The page refers
   to it from an `@font-face` inside the nonce'd `<style>`, never from a `<link>` (the page guard forbids `<link`).
4. An SVG carries no `<script>`, no event attributes and no external references; served with `image/svg+xml` and
   `nosniff` it is only ever an image.
5. Keep it small: everything here is in the binary and in every `cargo build`. A 24x24 icon is bytes; a subset font
   is tens of KB; anything bigger deserves a reason in the commit message.

The five `.woff2` files are the page's four families (Barlow Condensed 700, Geist Mono 400, Instrument Serif 400 italic, Inter 400 and 600), served as `font/woff2`. They are not edited here: they come from the front-end sources, which are not published here (official repository at a pinned commit, subset with pyftsubset, accepted by `ots`), and are copied here only after each sha256 checks out. Their rows in `THIRD_PARTY_NOTICES.md` carry the origin, `OFL-1.1` and the sha256 of the copy in this folder; the licence texts are in `licenses/fonts/` at the repository root.
