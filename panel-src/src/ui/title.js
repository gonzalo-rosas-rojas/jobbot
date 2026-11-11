// What the browser tab shows. The title is "Panel", always (A1, spec §3.8; only the public demo hands over its own name): the tab must not say what the panel is
// for, and the count of what is Gonza's belongs to the icon and to the HUD, never to the text. The icon is drawn on a canvas with
// the page's own tokens and its own Barlow Condensed (never a system font), and set as a blob: URL, which the CSP allows
// (img-src blob:); the previous one is revoked. A Rust test enforces the page's own <title>, this keeps the same rule at run time.

import { t } from "../i18n/index.js";
import { turnCount } from "../core/model.js";

const REVOKE_MS = 5000;                                                   // how long the previous icon stays alive after a new one is made

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** `brand` is the name the demo calls itself; the tab of the real panel gets none and says "Panel". */
export function createTitle(brand) {
  const icon = { link: null, url: null, key: "" };
  const fontReady = () => (document.fonts && document.fonts.load ? document.fonts.load('700 40px "Barlow Condensed"').catch(() => {}) : Promise.resolve());

  function paint(count) {
    if (String(count) === icon.key) return;
    icon.key = String(count);
    fontReady().then(() => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 64;
        canvas.height = 64;
        const g = canvas.getContext("2d");
        g.fillStyle = token("--night-2");
        g.beginPath();
        g.roundRect(2, 2, 60, 60, 12);
        g.fill();
        g.fillStyle = token("--text");
        g.font = '700 40px "Barlow Condensed", "Arial Narrow", sans-serif';
        g.textAlign = "center";
        g.textBaseline = "middle";
        g.fillText(count > 99 ? "99" : String(count), 32, 29);
        g.fillStyle = count > 0 ? token("--gold") : token("--text-dim");     // gold is "what is yours": the bar is only lit when something is
        g.fillRect(8, 49, 48, 7);
        canvas.toBlob((blob) => {
          if (!blob) return;
          // h() refuses <link> on purpose; this is the one place that needs it, and the address is our own blob.
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
        });
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
