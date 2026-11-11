// Boot order (spec §2.1, A1): language, theme, settings, sound, the session, the frame, the first picture, polling. The page paints the
// stage (sky, skyline, dial, the HUD and the stops bar, the sheet with the skeleton of its moment) in the first frame, without waiting for
// the snapshot; the snapshot only fills it. The address is read once, here, before the launch step takes the token (and the rest of the
// query) out of it.
//
// Two ways to show the same shell (a desktop window and a flat page, spec §3.11), chosen by one media query and switched without a reload:
//   scene/camera.js  Lenis + the scene engine + the stage      scene/flat.js  tabs, no stage, no canvas, no Lenis
//
// Query flags, for tests and screenshots:  ?debug=1 (window.__app)  ?clock=<hours> (a fixed sky)  ?lang=es|en  ?theme=light|dark
//   ?motion=calm|full  ?now=<iso date> (a fixed "now" for every "3 h ago" and countdown)  ?scenario=... (the demo's mood of the board,
//   scripts/demo/demo.js)  ?lag=<ms> (the demo's snapshot is slow)  ?poll=<ms> (with ?debug=1: a shorter refresh loop, from 500 ms)  ?embed

import { initLang, t, onLang } from "./i18n/index.js";
import { sound as engine } from "./sound/index.js";
import { createPrefs } from "./sound/prefs.js";
import { createPlayer } from "./sound/player.js";
import { createBackground } from "./sound/background.js";
import { createSession } from "./core/session.js";
import { readLaunch } from "./core/dom.js";
import { createView, restoreView, openRouteOnSnapshot } from "./core/store.js";
import { createSettings } from "./core/settings.js";
import { holdVisit, rememberVisit } from "./core/diff.js";
import { createShell } from "./ui/shell.js";
import { createClock, parseClock, horaOf } from "./scene/clock.js";
import { shortestDelta } from "./scene/poses.js";
import { createCamera } from "./scene/camera.js";
import { createFlat, watchFlat } from "./scene/flat.js";
import { createStage, clockText } from "./stage/stage.js";
import { createDial } from "./stage/dial.js";
import { mulberry32 } from "./stage/skyline.js";

history.scrollRestoration = "manual";
const search = location.search;
const params = new URLSearchParams(search);
const pinned = Date.parse(params.get("now") || "");
const flags = { debug: params.get("debug") === "1", clock: parseClock(search), now: Number.isFinite(pinned) ? pinned : null, poll: null };
const polled = Number(params.get("poll"));
if (flags.debug && Number.isFinite(polled) && polled >= 500) flags.poll = polled;          // for tests: the 30 seconds of the loop are too long to wait for

initLang();
holdVisit();                                                     // before anything can save a newer visit: the news of the time away are compared with this one
const launch = readLaunch();
const settings = createSettings({ search });
settings.set("sound", engine.enabled); // the engine owns the truth; the saved settings mirror it
settings.onChange(({ key, value }) => { if (key === "sound" && engine.enabled !== value) sound.setEnabled(value); });      // `sound` is the player (below): its listeners, the indicator, hear it

// A saved "sound on" becomes real with the first gesture of the page: that gesture is what lets the browser make the context.
function unlock() {
  window.removeEventListener("pointerdown", unlock, true);
  window.removeEventListener("keydown", unlock, true);
  if (engine.enabled && !engine.ctx) engine.setEnabled(true);
}
window.addEventListener("pointerdown", unlock, true);
window.addEventListener("keydown", unlock, true);

// Calm motion is the system's request or his own setting in SISTEMA: the engine, the stage and the sheet all ask this one function.
const system = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : { matches: false, addEventListener() {} };
const reduced = () => system.matches || settings.get("motion") === "calm";
// styles/ui/page.css cuts the transitions of CSS with this attribute, as the system request does
const writeMotion = () => { document.documentElement.dataset.motion = reduced() ? "calm" : "full"; };
writeMotion();
settings.onChange(({ key }) => { if (key === "motion") { writeMotion(); if (nav) nav.repaint(); } });

const view = createView();
const startRoute = restoreView(view, { embedded: launch.embedded, hash: location.hash });
let shell = null;
let nav = null;                                                  // the camera of a desktop window, or the flat page
const demo = window.__DEMO__ || null;                            // the public demo's in-memory server, absent on the real panel
const session = createSession({
  token: launch.token,
  demo,
  isTyping: () => !!shell && shell.isTyping(),
  isVisible: () => document.visibilityState === "visible",
  ...(flags.poll ? { pollMs: flags.poll } : {}),
});

// With ?now= the page's "now" is that instant; with ?clock= it moves to that hour of the day, so what a moment says ("HOY · 09:00",
// "vence en 25 h") agrees with the sky.
const shift = flags.clock == null ? 0 : shortestDelta(horaOf(Date.now()), flags.clock) * 3600000;
const now = flags.now != null ? () => flags.now : () => Date.now() + shift;

// The one `sound` of the page is the player: it stands between the page and the engine and decides whether a cue may sound (the buses, a silence, working
// hours, calm motion; sound/player.js). The background detector turns what changed between two snapshots into one quiet cue (sound/background.js).
// The clock is the page's, so ?clock= moves the working hours with the sky.
const sound = createPlayer({ engine, prefs: createPrefs(), now, reduced });
const background = createBackground({ player: sound, now });

shell = createShell({ root: document.getElementById("shell"), view, session, settings, sound, flags, embedded: launch.embedded, reduced, now, brand: demo && demo.brand, about: demo && demo.about });
session.subscribe((event) => {
  if (event.type === "snapshot") background.observe(event.snapshot);
  else if (event.type === "error") background.failed(event.error);
});

// ---- the hour: the real one, or ?clock=
const clock = createClock({
  now,
  override: flags.clock,
  onMinute: () => { shell.setClock(clockText(clock.hora())); shell.tick(); background.tick(); nav.repaint(); },
  onCross: (mark) => { shell.announceCross(mark); nav.repaint(); },
});
shell.setClock(clockText(clock.hora()));
clock.start();

// ---- the stage (built once; a flat page never draws it)
const dialEl = document.getElementById("dial");
const dial = createDial(dialEl, { t });
const stage = createStage({
  canvas: document.getElementById("sky"), dialHost: dialEl, t, parts: { dial }, rand: mulberry32(20261007),
  getSheetRect: () => shell.app.getBoundingClientRect(),
});
const camera = createCamera({ shell, stage, clock, reduced, canSnap: shell.canSnap });
const flat = createFlat({ shell });

// ---- one shell, two navs: the window decides, and the page changes its mind without a reload
function useNav(isFlat) {
  const at = shell.current() || view.room;
  const next = isFlat ? flat : camera;
  if (shell.drawer.isOpen()) shell.drawer.close({ silent: true });
  if (nav && nav !== next) nav.stop();
  nav = next;
  shell.setNav(nav, isFlat);
  nav.start(at);
}
const watch = watchFlat({ onChange: useNav });
useNav(watch.flat());
// The address says where the page is; it never travels to get there (an address that names an application keeps it until the drawer opens).
if (!(startRoute && startRoute.job)) shell.router.write(shell.current(), null);
onLang(() => { dial.relabel(); nav.repaint(); });
system.addEventListener("change", () => { writeMotion(); nav.repaint(); });
// a hidden page asks nothing (the loop is stopped, a read in flight is let finish); coming back reads at once
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") { session.wake(); clock.tick(); }
  else { session.pause(); rememberVisit(); }
});
window.addEventListener("pagehide", () => rememberVisit());

// ---- the first picture is painted; now the data
openRouteOnSnapshot(session, startRoute, (job, room) => shell.openJob(job, room));    // with the first snapshot that is accepted, even when the first read failed
session.start();

if (flags.debug) {
  window.__app = {
    session, sound, background, settings, shell, view, clock, stage,
    model: () => session.model,
    go: shell.go,
    drawer: shell.ctx.drawer,
    get engine() { return camera.engine; },          // null on a flat page
    get lenis() { return camera.lenis; },
    get scene() { return camera.S; },
    mode: () => (nav === flat ? "flat" : "stage"),
  };
}
