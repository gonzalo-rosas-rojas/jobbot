// What the browser tab shows. The title is "Panel", always (A1, spec §3.8; only the public demo hands over its own name): the tab must not say what the panel is
// for, and the count of what is Gonza's belongs to the icon and to the HUD, never to the text. The icon is drawn on a canvas with
// the page's own tokens and its own Barlow Condensed (never a system font), and set as a blob: URL, which the CSP allows
// (img-src blob:); the previous one is revoked. A Rust test enforces the page's own <title>, this keeps the same rule at run time.

import { t } from "../i18n/index.js";
import { turnCount } from "../core/model.js";

const REVOKE_MS = 5000;                                                   // how long the previous icon stays alive after a new one is made

// The icon is a 64 px square: a rounded plate, the count in its middle and a bar under it.
const ICON_SIZE = 64;
const PLATE = { x: 2, y: 2, size: 60, radius: 12 };
const COUNT_FONT = '700 40px "Barlow Condensed", "Arial Narrow", sans-serif';
const COUNT_AT = { x: 32, y: 29 };
const BAR = { x: 8, y: 49, w: 48, h: 7 };

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** `brand` is the name the demo calls itself; the tab of the real panel gets none and says "Panel". */
export function createTitle(brand) {
  const icon = { link: null, url: null, key: "" };
  const fontReady = () => (document.fonts && document.fonts.load ? document.fonts.load('700 40px "Barlow Condensed"').catch(() => { /* without the font the icon is drawn with the fallback one: still an icon */ }) : Promise.resolve());

  /** Draws the icon for `count` on a canvas and hands its blob to `done`. */
  function drawIcon(count, done) {
    const canvas = document.createElement("canvas");
    canvas.width = ICON_SIZE;
    canvas.height = ICON_SIZE;
    const g = canvas.getContext("2d");
    g.fillStyle = token("--night-2");
    g.beginPath();
    g.roundRect(PLATE.x, PLATE.y, PLATE.size, PLATE.size, PLATE.radius);
    g.fill();
    g.fillStyle = token("--text");
    g.font = COUNT_FONT;
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.fillText(count > 99 ? "99" : String(count), COUNT_AT.x, COUNT_AT.y);
    g.fillStyle = count > 0 ? token("--gold") : token("--text-dim");     // gold is "what is yours": the bar is only lit when something is
    g.fillRect(BAR.x, BAR.y, BAR.w, BAR.h);
    canvas.toBlob((blob) => { if (blob) done(blob); });
  }

  /** Makes the blob the tab's icon and lets go of the previous one. */
  function setIcon(blob) {
    // h() refuses <link> on purpose; this is the one place that needs it, and the address is our own blob.
    // The published demo already declares one in its markup (a static host has no server to answer /favicon.ico): the tab keeps one icon, so that link gets the blob.
    if (!icon.link) icon.link = document.querySelector('link[rel="icon"]');
    if (!icon.link) {
      icon.link = document.createElement("link");
      icon.link.rel = "icon";
      document.head.append(icon.link);
    }
    const old = icon.url;
    icon.url = URL.createObjectURL(blob);
    icon.link.href = icon.url;
    // not at once: the browser may still be reading the previous one as the tab's icon, and a revoked blob it asks for is a failed request in the console
    if (old) setTimeout(() => URL.revokeObjectURL(old), REVOKE_MS);
  }

  function paint(count) {
    if (String(count) === icon.key) return;
    icon.key = String(count);
    fontReady().then(() => {
      try {
        drawIcon(count, setIcon);
      } catch (e) { /* the icon is a courtesy */ }
    });
  }

  return {
    /** Keeps the title plain and draws the icon for this snapshot's model; a null model (before the first picture) is a zero. */
    update(model) {
      const base = brand || t("app.title");
      if (document.title !== base) document.title = base;
      paint(model ? turnCount(model) : 0);
    },
  };
}
