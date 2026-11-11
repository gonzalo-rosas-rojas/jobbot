# Vendored and third-party material

Everything in the panel that did not come from this repository, where it came from, and the checks that keep it honest.
Policy: `docs/research/2026-10-07-revision/R8-reutilizar-con-seguridad.md` section 2 (ten rules) and R8b section 7 (the
checklist); versions, licences and weights: R8a. Two doors only: code enters by **npm**, pinned and hashed; fonts, icons and
other files enter by **`assets/`**, with a row here (or in `assets/fonts/VENDORED.md`, which `tools/fonts.mjs` writes).
What is in neither list does not exist for the bundle. `THIRD_PARTY_NOTICES.md` carries the licence texts (esbuild strips
the comments from the bundle); `tools/notices.mjs` writes it from the licence files really installed.

Written 2026-10-08. Nothing below is executed from where it was downloaded: downloads land in `.vendor-src/<timestamp>/`
(git ignores it), are checked there and processed from outside it with absolute paths.

## What to run

| Command | What it proves |
|---|---|
| `npm ci --ignore-scripts` | installs exactly the lock, with `.npmrc` (`save-exact`, `ignore-scripts`, no audit, no fund, no update notifier) |
| `node check-lock.mjs` | versions exact, sha512 of every tarball against the table in the script (taken from the registry separately), nothing unapproved in the lock, one install script (esbuild), licences on the allow-list (read from the installed packages), 7 day quarantine, notices present, then the registry asked again for hash, date, licence and deprecation. `--offline` skips the last step |
| `npm audit signatures` | registry signatures (9 of 9 verified on 2026-10-08) and provenance (3 attested: esbuild, its win32-x64 binary, lucide-static) |
| `npm query ":attr(scripts, [postinstall])"` | exactly `esbuild` (and no `preinstall` anywhere) |
| `node tools/audit-deps.mjs` | the grep for network, dynamic code and markup sinks in the files that reach the page (below) |
| `node tools/fonts.mjs check [--online]` | the five fonts and four licences against `assets/fonts/VENDORED.md` and the manifest; `--online` downloads the originals again |
| `node tools/icons.mjs --check`, `node tools/notices.mjs --check` | the sprite and the notices are what their generators write |
| `node tools/weigh.mjs` | what each library costs, gzipped |
| `npm test` | all of the above as tests |

## npm packages

All MIT except lucide-static (ISC). `published` is the registry's date for that exact version, `adopted` the day it entered the
project; the gap is the quarantine (7 days, `check-lock.mjs`, because npm 11.1 has no `min-release-age`; add
`min-release-age=7` to `.npmrc` when npm is 11.10 or later). The tarball sha512 was read with `npm view <pkg>@<version>
dist.integrity` apart from the install, and equals the lock.

| package | version | licence | published | adopted | reaches | sha512 |
|---|---|---|---|---|---|---|
| esbuild | 0.28.2 | MIT | 2026-08-08 | 2026-10-07 | build tool | `sha512-HKVLS8dvII+xoKW9kmqxbRKrnWEXfJJr/FZhhJmiqIB0e053QNYFqOBouTMO/k5sID4MvCiUCvv8b9M4h32wIA==` |
| @esbuild/win32-x64 | 0.28.2 | MIT | 2026-08-08 | 2026-10-07 | build tool (this machine's binary; the other platforms' binaries are in the lock too, each with its hash) | `sha512-5ebpxr3nWMzrL/rnUI755Jkuee0bHL/Gq0WTF9lvcpv73wAp5eu8MfBUgWK9bhWvZjj7yX8etf/8tI8Ney695g==` |
| lenis | 1.3.26 | MIT | 2026-08-05 | 2026-10-08 | page | `sha512-s/xTCZCxTFvHbAN1OzuhNaN5YPJH2ail0XAkctKW1b+RUAG4nUL5UHLXwNko1h8aEeT2jspBXegMgPJd8zcuag==` |
| tinykeys | 4.0.1 | MIT | 2026-09-25 | 2026-10-08 | page | `sha512-rClxhLQl+I5UMB5Ny7sfnZqDNa51u7h7h4gJV2UWslwADHEw75IuVPCnWrpO/otM7jHrHpwcZdRHd+//SiI55A==` |
| @leeoniya/ufuzzy | 1.0.19 | MIT | 2025-08-22 | 2026-10-08 | page | `sha512-0pikDeYt0IHEUPza5RTCDXc/17S1pTrYnReEMp8Aa6k1ovzw5QdZLwicW8TjljwEZRb6oYag0xmALohrcq/yOQ==` |
| lucide-static | 1.49.0 | ISC | 2026-09-29 | 2026-10-08 | build (only the SVG shapes of 33 icons, see Icons) | `sha512-gSjtl7bR2v26oDInh2Wua1PrF2BRRXl9inhnLWcFfnRqRShmgZvlFRisPHqTfU7kxsuZU8ThNjAFotbInrzKow==` |

Decisions worth knowing:

- **lucide-static is 1.49.0, not R8a's 1.53.0.** 1.53.0 was published the same day (2026-10-08); the quarantine took the
  newest release at least 7 days old (1.49.0, 2026-09-29; the later ones are 1.50 to 1.52). It is a `devDependency`: only
  `tools/icons.mjs` reads it. Move to a newer one after the week, never in a behaviour commit.
- **No package has dependencies.** lenis declares optional peers (react, vue, nuxt) that are not installed. The only install script
  in the lock is esbuild's `postinstall`, which esbuild's own documentation says is not needed when scripts are off (the platform
  binary is chosen from the optional packages).
- **Pins have two sources**: the lock and the table in `check-lock.mjs`. Raising a version means reading `npm diff`, waiting out
  the quarantine, `npm view`, editing the table and `THIRD_PARTY_NOTICES.md`, and keeping it out of any behaviour change.
- **Not installed on purpose**: GSAP (licence is not free software), Motion, ogl, ninja-keys (Lit builds markup with
  `innerHTML`), Fuse.js, glyphhanger, subfont (R8a). `osv-scanner` and `cargo audit` are not on this machine; `npm audit`
  found 0 vulnerabilities.

## What the dependencies do (tools/audit-deps.mjs)

The files esbuild reads for lenis, tinykeys and @leeoniya/ufuzzy, with comments removed, searched for
`fetch(`, `XMLHttpRequest`, `WebSocket`, `EventSource`, `sendBeacon`, `Worker(`/`importScripts`, `import(`, `eval`,
`new Function`, `document.write`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `srcdoc`, `location`, `document.cookie`,
Web Storage and IndexedDB, `navigator` and any `http(s)://` text. 4 hits, every one read:

| package | hit | context | verdict |
|---|---|---|---|
| lenis | `window.location.href` (1) | `onClick` builds `new URL(...)` to tell an in-page anchor from another page; only runs when the options `anchors` or `stopInertiaOnNavigate` are on | harmless; we do not turn those options on, and nothing is written |
| lenis | `navigator.userAgent` (1) | tested against `iPad\|iPhone\|iPod` to set `isIos` | harmless, read-only |
| tinykeys | `navigator` (2, one line) | `typeof navigator === "object" ? navigator.platform : ""` picks Meta or Control for `$mod` | harmless, read-only |
| @leeoniya/ufuzzy | none | | |

Found by reading rather than by the grep: lenis sets two globals (`window.lenis`, `window.lenisVersion`) and puts the classes
`lenis`, `lenis-smooth`, `lenis-stopped` on `<html>`; it registers non-passive `wheel` and `touch*` listeners on the window
(that is how it smooths) and skips elements marked `data-lenis-prevent`. uFuzzy's default `highlight` builds a `"<mark>"` string
for `innerHTML`; the page never calls it: `src/moments/procesos/search.js` (`partsOf`) cuts the text at uFuzzy's ranges into plain objects and the
page appends text nodes and `<mark>` elements. No package contains a URL outside comments. three.js is not a dependency any more
(its tower is archived, `guard.mjs` refuses it in the bundle).

## Fonts

Five woff2 faces (Barlow Condensed 700, Instrument Serif 400 italic, Geist Mono 400, Inter 400 and 600), OFL-1.1, 99.7 KB
together (budget 110 KB, R8 rule 10 caps it at 120). Origins, pinned commits, URLs, both fingerprints of every original, the
subset commands and the OTS verdict per face: `assets/fonts/VENDORED.md`, written by `node tools/fonts.mjs`. The tools are pinned
(fonttools 4.66.1, brotli 1.2.0, opentype-sanitizer 9.2.0 through `uvx`) and run with `PYTHONHASHSEED=0` and
`SOURCE_DATE_EPOCH` fixed, so the same inputs give the same bytes anywhere. They are not connected to the page yet: that needs
`font-src 'self'` and the asset route in Rust (R8 section 3), which belong to the next phase.

## Icons

`assets/icons.svg` is a sprite of 33 Lucide icons (list in `tools/icons.mjs`): 9,260 bytes, 1,760 bytes gzipped, the largest symbol 523 bytes. It is built by `node tools/icons.mjs` from `node_modules/lucide-static/icons/<name>.svg`; the parser accepts only
`path circle rect line polyline polygon ellipse` with geometry attributes and throws on anything else (script, style,
foreignObject, handlers, hrefs). Every `<symbol>` carries `viewBox="0 0 24 24" fill="none" stroke="currentColor"
stroke-width="2" stroke-linecap="round" stroke-linejoin="round"`, so colour and size come from the page's CSS.

How the next phase puts it on the page (inline, hidden, no request):

1. Put the marker `<!--__ICONS__-->` at the start of `<body>` in `page.template.html`; `build.mjs` replaces it with the text of
   `assets/icons.svg` (it already does this for labs: see `buildLab`). The sprite has no `xmlns` and no `http` text, so
   `guard.mjs` is satisfied, and it is `hidden` with `width="0" height="0"`.
2. Draw an icon with `<svg class="ico" aria-hidden="true"><use href="#i-sun"/></svg>`; in code, `sv("svg", { class: "ico", "aria-hidden": "true" }, sv("use", { href: "#i-sun" }))`
   (`core/dom.js` lets `#` hrefs through). CSS: `.ico { width: 1.25em; height: 1.25em; flex: none; }`; colour is `currentColor`.
3. Adding an icon: put its Lucide name in `ICONS`, run `node tools/icons.mjs`, run the tests. No notice changes (the Lucide
   licence text, ISC plus the MIT part for the Feather-derived icons, is already in `THIRD_PARTY_NOTICES.md`).

Checked in Chrome under the page's CSP and under the target CSP with Trusted Types: all 33 draw.

## Lenis stylesheet

`node_modules/lenis/dist/lenis.css` (0.5 KB; every rule starts with `.lenis`) goes into the page right after `base.css`: build.mjs reads the file
from node_modules (no copy to drift) and minifies it.

## Weights (tools/weigh.mjs, gzip level 9, the project's esbuild settings)

| library (the import the page makes) | raw | gzip | budget |
|---|---|---|---|
| lenis | 18.2 KB | 5.3 KB | 7.0 KB |
| tinykeys | 1.6 KB | 0.9 KB | 2.0 KB |
| uFuzzy | 8.7 KB | 4.2 KB | 6.0 KB |
| **together** | **28.5 KB** | **10.4 KB** | 14.0 KB |

Fonts 99.7 KB, sprite 1.7 KB gzipped.

## Not done yet

- `min-release-age` in `.npmrc` (needs npm 11.10; the script covers it meanwhile).
- `npm sbom --sbom-format cyclonedx` is not generated or committed; `osv-scanner` and `cargo audit` are not installed (R8 question 6).
- Fonts are not served by Rust and the CSP has no `font-src`; the sprite and Lenis are not in the page template. All of that is
  the next phase.
