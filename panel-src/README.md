# panel-src: the panel's front end ("El Turno Continuado")

The page the Rust binary serves, written as ES modules and built into ONE file by esbuild: a sky, a skyline and a dial of the day behind ONE glass
sheet, six stops (Anoche, Hoy, Procesos, Cola, Aprender, Sistema) that the camera travels between. Concept and contracts: the design spec,
and how the chassis and each stop were built: its plans (neither is published with this repository); what the old page did: `legacy/FEATURES.md`; rules for anyone editing here:
`AGENT-RULES.md`; why each big decision stands: `docs/adr/`. The first tower (three.js world, five rooms, v2) is kept in an archive folder that is not part of this repository: it is not
built and not tested, and `test/archive.guard.test.mjs` fails if anything that builds or runs imports it again.

## Build, run, test

```
node build.mjs demo --out dist/demo-mine.html   # the page + fictional data (scripts/demo/demo.js); --out keeps parallel agents apart
node tools/serve.mjs 8795                       # serves dist/ under the real CSP: http://127.0.0.1:8795/dist/demo-mine.html?debug=1
node build.mjs demo --publish --out dist/demo-pub/index.html   # the published demo: a folder (the page, its five fonts and their licences) that a static host serves as it is; guard.mjs publishedProblems(page) checks the file alone
node build.mjs                                  # the real page -> dist/page.html, checked by guard.mjs (add --install for ../crates/jobbot-panel/src/page.html)
node build.mjs lab f0-hoy                       # a lab page (lab/<name>.html + lab/<name>.entry.js)
npm test                                        # every test/**/*.test.mjs; browser tests start their own Chrome and server
npm run check                                   # what to run before handing in: build + guard, layers, bidi, every test without a browser (stops at the first failure)
npm run check:full                              # the same plus the browser tests, one file at a time (tools/check.mjs --list says which is which)
```

Query flags (read once at boot, then the address is cleaned): `?debug=1` exposes `window.__app` (`session`, `model()`, `sound`, `settings`,
`shell`, `view`, `clock`, `stage`, `engine` and `lenis` (null on a flat page), `mode()` ("stage" or "flat"), `go(stop)`, `drawer`),
`?clock=<hours>` (a fixed sky, and the page's "now" moves to that hour), `?now=<iso date>` (a fixed "now" for every "3 h ago" and countdown),
`?motion=calm|full`, `?lang=es|en`, `?theme=light|dark`, `?embed` (never touches the history). The demo also reads
`?scenario=calm|urgent|stop|stale|heavy|empty|window|closed` (the mood of the board) and `?lag=<ms>` (every snapshot read of the demo takes that long,
and `window.__DEMO__.stats` counts the reads and how many were in flight at once). With `?debug=1`, `?poll=<ms>` (from 500) shortens the refresh loop. The
tests of the whole page are `test/page.*.browser.test.mjs` (helper: `test/helpers/page.mjs`: the page built once per process, a private Chrome, the server
held back or not).

The first snapshot of a big database can take seconds, so the page never waits for it: the sky, the HUD, the stops and the dial are drawn at once and the
sheet says "Cargando tus datos…" (aria-busy, aria-live polite; after 3 s a second quiet line) with no figure it does not know; a failed first read is the
banner with a "Reintentar" button. The refresh loop (`core/session.js`) keeps one read in flight, cuts a read that never answers (60 s, AbortController),
counts its 30 s from the END of the last read, reads once after an action, asks nothing while the tab is hidden and reads at once when it comes back.

## Foundations (the page builds on these)

Fonts, icons and small libraries entered under the supply-chain policy that `check-lock.mjs` enforces (it comes from R8 of the 2026-10-07 revision, which
lives in the main checkout, not on this branch). The page bundles three libraries (Lenis, uFuzzy, tinykeys); esbuild builds it and lucide-static feeds the icon sprite
(three, anime.js and simplex-noise were dropped with the foundations lab, `baaebaf` and `1a02e72`; `docs/adr/0008-dependencies-enter-through-check-lock.md` says why).
Everything that is not ours is listed in `VENDORED.md` (npm packages, hashes, what each does on the network, weights) and
`assets/fonts/VENDORED.md` (origins, pinned commits, both fingerprints); the licence texts are in `THIRD_PARTY_NOTICES.md`.

```
npm ci --ignore-scripts                 # installs exactly the lock (.npmrc: exact versions, no scripts, no calls home)
node check-lock.mjs                     # the supply-chain gate: hashes, quarantine, licences, one install script; --offline skips the registry
node tools/fonts.mjs [check [--online]] # downloads, verifies, subsets and sanitises the five fonts into assets/fonts/ (or only checks them)
node tools/icons.mjs                    # assets/icons.svg, a sprite of 33 Lucide icons (geometry only)
node tools/notices.mjs                  # THIRD_PARTY_NOTICES.md from the installed licence files
node tools/audit-deps.mjs               # what the dependencies do with the network, strings-as-code and markup
node tools/weigh.mjs                    # gzip cost of each library
```

The pure parts the page builds on live in `src/scene/jump.js` (the R7 jump: `min(800, 420 + 120 * (N - 1))` ms, inOutQuart, planned and stepped as
pure functions) and `src/ui/keyfilter.js` (when a shortcut stays quiet). The obviously fake postings of the heavy fixture are `test/fixtures/fake.mjs`.

## Where each concern lives

- `src/main.js` boot order: language, settings, sound unlock, the session, the shell, the clock, the stage, then ONE of two navigators chosen by a
  media query (`scene/camera.js` on a window, `scene/flat.js` under 760 px; the page changes between them on resize, without a reload), polling.
- `src/ui/shell.js` the frame: builds the page (skip link, HUD with the stops bar, the glass sheet `main#app`, the drawer `aside#detail`, the bottom
  bar, the toast, the one live region), keeps ONE moment mounted, wires every snapshot (counts, title and icon, the moment), and sends every text of
  the server through `core/humanize.js` on its way to the toast, the live region and the banner. `ui/hud.js`, `ui/stops.js`, `ui/bar.js`,
  `ui/router.js` the address, `ui/keys.js` the one key table, `ui/wired.js` which keys each stop answers (the help lists only those),
  `ui/shell-keys.js` who owns each verb, `ui/tools.js` theme, language, sound, `ui/cursor.js` j/k, `ui/menu.js`, `ui/help.js`, `ui/palette.js` (Ctrl K: a
  modal of dark glass that finds and takes you there, with `ui/palette-items.js` what it holds and `ui/palette-rank.js` how a query ranks it, both pure; it
  changes nothing on the server), `ui/drawer.js` (container) with `ui/detail.js` (content, parts in `ui/detail/`), `ui/toast.js`, `ui/announce.js`, `ui/title.js`.
- `src/scene/` the camera: `engine.js` (one variable, the position between the stops; two writers of the scroll, one reader), `jump.js`, `poses.js`
  (the sheet's opacity and geometry), `clock.js` (the hour of the sky), `camera.js` (Lenis + the engine + the painter) and `flat.js`.
- `src/stage/` the picture behind the sheet: `sky.js`, `skyline.js`, `dial.js`, `beam.js` and `stage.js` that puts them together on one canvas.
- `src/moments/registry.js` the six stops; `src/moments/<id>.js` one file each, with the stop's parts in `src/moments/<id>/`
  (prologo, hoy, procesos, cola, aprender, sistema).
- `src/core/` pure logic (`model`, `session`, `store`, `settings`, `diff`, `format`, `time`, `humanize`, `dom`).
- `src/i18n/{es,en}/` one dictionary per scope (`common`, `shell`, one per moment), identical keys in both languages.
- `src/sound/` the synthesised sounds; `src/styles/` tokens, base, stage, dial, sheet, moments, `ui/` (one file per moment, `page.css`).

## Add or replace a moment

Write `src/moments/<id>.js` with `export default { id, mount(host, ctx), update(model, ctx), unmount() }` (and, when it has them, `onKey(actionId,
row, event)`, `arrive(opts)` and `drawer(detail, card, ctx)`; the contract is in `AGENT-RULES.md` and in the header of `registry.js`) and change its line in
`MOMENTS`. A NEW stop (not the replacement of one) touches more files than that: the list is in `AGENT-RULES.md`, "Adding or changing a stop". Put
its words in `src/i18n/{es,en}/<id>.js` and its styles in `src/styles/ui/<id>.css` (every file of that folder that does not start with `_` goes into
the page). Add the ids of the keys it answers to its list in `ui/wired.js`: the help promises nothing else. Build DOM only with `h()`/`sv()`, and mark a
region that has keys of its own (digits, letters) with `data-local-keys` so the global shortcuts step aside inside it. The drawer's content is
`renderDetail(detail, card, ctx)` in `ui/detail.js` (signature in its header); a moment may bring its own page of an application with `drawer()`.

## Fonts and images (assets)

The page is one file, but the binary can also serve a few files of its own (`crates/jobbot-panel/src/assets/`, listed in the `ASSETS`
table of `crates/jobbot-panel/src/http.rs`, each with a row in `THIRD_PARTY_NOTICES.md`; the rule for adding one is in
`crates/jobbot-panel/src/assets/README.md`). They are reached through the `__ASSETS__` placeholder, which the server fills in at every
load exactly like `__NONCE__` (it becomes `/a/<launch-id>`, a random segment per start):

```css
@font-face { font-family: "Name"; src: url("__ASSETS__/name.woff2") format("woff2"); font-display: swap; }
.picture { background: url("__ASSETS__/picture.svg") center / contain no-repeat; }
```

Rules: fonts go through `@font-face` inside the nonce'd `<style>` (the page guard forbids `<link`), never `@import`, never
an absolute address (a Rust test requires every `url(` in the page to start with `__ASSETS__/` or `#`). The CSP allows
`font-src 'self'` and `img-src 'self' blob:` and nothing else, and `require-trusted-types-for 'script'` makes a string
handed to `innerHTML`, `eval` or a script's `src` throw. Scripts cannot see the placeholder (it only exists in text the
server rewrites); to use an asset from JavaScript, declare it in CSS (`--picture: url("__ASSETS__/picture.svg")`) and read
the custom property, or name the element and let CSS paint it. Today the only assets are the five fonts. `tools/serve.mjs`
fills `__ASSETS__` like the real server (under a random `/a/<launch-id>/`), so pages and labs show their fonts in development.

## Habits that keep it honest

No `innerHTML`, no inline `style=""`, no network except `/api/*`, no strings of user text outside the dictionaries (a test
scans `src/ui`). Budgets (`guard.mjs`, they break the build): 480 KB of script MINIFIED (the length of the minified bundle) and 200 KB gzipped for the
whole page (script, style and icon sprite); a jump to the next stop takes 420 ms. The build also prints the script by area (layer, moment, dictionary, library)
and refuses growth past a margin over `budgets.json`; `node build.mjs --update-budgets` accepts it on purpose. How the caps got there, and why they are not
raised casually, is in `docs/adr/0005-budgets-and-their-history.md`. Why the page is built the way it is: `docs/adr/README.md`.
