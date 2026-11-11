// Lab "f0-small": the toast, the tab title and icon, the keyboard cursor and the sheet's CSS, on a page with the real skeleton
// (doc, stage, shell, main#app.sheet). Driven through window.__lab by test/ui.small.browser.test.mjs.
//   toast:   __lab.toast.show/polite/hide, __lab.tick(ms) (the toast's timers are fake: 4 s costs nothing), __lab.said, __lab.toastState()
//   title:   __lab.title("rich" | "empty" | null), __lab.icon()  -> the pixels of the icon the tab got
//   cursor:  __lab.cursor, __lab.view, __lab.redraw(), __lab.keyed()
//   sheet:   __lab.moment("hoy" | "procesos" | "prologo"), __lab.rows(n)
import { h } from "../src/core/dom.js";
import { model } from "../src/core/model.js";
import { createToast } from "../src/ui/toast.js";
import { createTitle } from "../src/ui/title.js";
import { createCursor } from "../src/ui/cursor.js";
import rich from "../test/fixtures/snapshot.rich.json";
import empty from "../test/fixtures/snapshot.empty.json";

// Fake timers for the toast: tick(ms) runs what is due.
const timers = new Map();
let nextId = 1;
let clock = 0;
const set = (fn, ms) => { const id = nextId++; timers.set(id, { at: clock + ms, fn }); return id; };
const clear = (id) => timers.delete(id);
const tick = (ms) => { clock += ms; for (const [id, timer] of [...timers]) if (timer.at <= clock) { timers.delete(id); timer.fn(); } };

const said = [];
const toast = createToast({ say: (text) => said.push(text), set, clear });
document.getElementById("shell").append(toast.el);
const title = createTitle();

const view = { cursor: null };
const ticks = [];
const cursor = createCursor({ view, sound: { play: (name) => ticks.push(name) } });
const list = document.getElementById("rows");
function rows(n = 30) {
  list.replaceChildren(...Array.from({ length: n }, (_, i) => h("li", { class: "row", dataset: i % 2 ? { task: "t" + i } : { card: "k" + i } }, "row " + i)));
  cursor.apply();
}
rows();

window.__lab = {
  toast, tick, said,
  toastState: () => ({
    cls: toast.el.className, text: toast.el.querySelector(".toast-text").textContent, timers: timers.size,
    undo: !toast.el.querySelector(".toast-undo").hidden, close: !toast.el.querySelector(".toast-close").hidden,
    undoText: toast.el.querySelector(".toast-undo").textContent, live: toast.el.getAttribute("aria-live"), role: toast.el.getAttribute("role"),
  }),
  title: (which) => title.update(which === "rich" ? model(rich) : which === "empty" ? model(empty) : null),
  icon: async () => {
    const link = document.querySelector('link[rel="icon"]');
    if (!link || !link.href.startsWith("blob:")) return null;
    const img = new Image();
    img.src = link.href;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 64;
    const g = canvas.getContext("2d", { willReadFrequently: true });
    g.drawImage(img, 0, 0);
    const px = (x, y) => Array.from(g.getImageData(x, y, 1, 1).data).slice(0, 3);
    return { href: link.href, tile: px(10, 10), stripe: px(32, 52) };
  },
  cursor, view, ticks,
  redraw: () => { const keep = view.cursor; rows(); view.cursor = keep; cursor.apply(); },
  keyed: () => [...document.querySelectorAll('[data-cursor="true"]')].map((el) => el.textContent),
  rows,
  moment: (name) => { document.getElementById("app").dataset.moment = name; },
};
