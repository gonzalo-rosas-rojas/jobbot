// The cue under PROCESOS' list (moments/procesos/scroll-cue.js) over plain stand-ins for the nodes: one measure per frame however often it is
// asked for, the three flags of the viewbox, and the thumb only when the list scrolls.
import test from "node:test";
import assert from "node:assert/strict";
import { setLang } from "../src/i18n/index.js";
import { createScrollCue } from "../src/moments/procesos/scroll-cue.js";

const rect = (top, bottom) => ({ top, bottom, left: 0, right: 0, width: 0, height: bottom - top });

function rig({ scrollHeight, clientHeight, scrollTop = 0, cards = [] }) {
  setLang("es");
  const frames = [];
  const style = new Map();
  const scroller = {
    scrollHeight, clientHeight, scrollTop,
    listeners: new Map(),
    addEventListener(type, fn) { this.listeners.set(type, fn); },
    removeEventListener(type, fn) { if (this.listeners.get(type) === fn) this.listeners.delete(type); },
    getBoundingClientRect: () => rect(0, clientHeight),
    querySelectorAll: () => cards.map((top) => ({ getBoundingClientRect: () => rect(top, top + 10) })),
    querySelector: () => null,
  };
  const viewbox = { dataset: {} };
  const cueEl = { textContent: "" };
  const thumb = { style: { setProperty: (k, v) => style.set(k, v) } };
  const cue = createScrollCue({ scroller, viewbox, cueEl, thumb, observed: [], reduced: () => true, raf: (fn) => frames.push(fn) });
  return { cue, frames, viewbox, cueEl, style, scroller };
}

test("asking to measure many times costs one frame", () => {
  const { cue, frames } = rig({ scrollHeight: 100, clientHeight: 100 });
  cue.queue(); cue.queue(); cue.queue();
  assert.equal(frames.length, 1);
  frames[0]();
  cue.queue();
  assert.equal(frames.length, 2, "after the frame ran, the next ask queues a new one");
  cue.destroy();
});

test("a list that fits does not scroll: no thumb, and it is at both ends", () => {
  const { cue, frames, viewbox, style } = rig({ scrollHeight: 100, clientHeight: 100 });
  cue.queue();
  frames[0]();
  assert.equal(viewbox.dataset.canScroll, "false");
  assert.equal(viewbox.dataset.atStart, "true");
  assert.equal(viewbox.dataset.atEnd, "true");
  assert.equal(style.size, 0);
  cue.destroy();
});

test("a taller list scrolls: it counts the piles below the box and sizes the thumb", () => {
  const { cue, frames, viewbox, cueEl, style } = rig({ scrollHeight: 400, clientHeight: 100, cards: [10, 60, 120, 220] });
  cue.queue();
  frames[0]();
  assert.equal(viewbox.dataset.canScroll, "true");
  assert.equal(viewbox.dataset.atStart, "true");
  assert.equal(viewbox.dataset.atEnd, "false");
  assert.match(cueEl.textContent, /2 más/, "two piles start below the bottom edge of the box");
  assert.equal(style.get("top"), "0.00%");
  assert.equal(style.get("height"), "25.00%");
  cue.destroy();
});

test("after destroy a scroll queues nothing", () => {
  const { cue, frames, scroller } = rig({ scrollHeight: 100, clientHeight: 100 });
  scroller.listeners.get("scroll")();
  assert.equal(frames.length, 1, "while alive, a scroll queues a frame");
  cue.destroy();
  assert.equal(scroller.listeners.has("scroll"), false, "the listener is removed");
});
