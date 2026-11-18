// COLA's render (moments/cola/render.js) without a browser, over test/helpers/fake-dom.mjs: what a repaint keeps of a row, and the name a button gives.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installFakeDom } from "./helpers/fake-dom.mjs";

const dom = installFakeDom();
const { setLang, setTimeZone } = await import("../src/i18n/index.js");
const { model } = await import("../src/core/model.js");
const { h } = await import("../src/core/dom.js");
const { colaView } = await import("../src/moments/cola/view.js");
const { createCola } = await import("../src/moments/cola/render.js");

const snap = JSON.parse(readFileSync(new URL("./fixtures/snapshot.rich.json", import.meta.url), "utf8"));
const nowMs = Date.parse(snap.generated_at);

const proposal = (n, over = {}) => ({
  key: "p:" + n, company: "Company " + n, title: "Role " + n, score: 70 + n, state: "prepared", source: "ashby", url: "https://example.com/p/" + n,
  reason: null, reason_kind: null, reason_short: null, note: null, approved: false, has_screenshot: true, has_cv: true, has_dossier: false,
  updated_at: "2026-10-07T06:00:00Z", actions: ["approve"], marks: [], posted_at: "2026-10-02T15:00:00Z", first_seen: "2026-10-03T15:00:00Z",
  salary_min: 5000, salary_max: 6500, days_in_state: 1, snoozed_until: null, waiting_since: null, age_days: 4, prune_in_days: 10, ...over,
});

function rig(cards) {
  setLang("es");
  setTimeZone("America/Santiago");
  dom.reset();
  const actions = { armedOf: () => null, isPending: () => false, engineArmed: () => false, batch: { isOpen: () => false, picks: () => [] } };
  const shot = { el: h("figure"), show() {}, destroy() {} };
  const page = createCola({ actions, shot, on: {}, reduced: () => true });
  dom.document.body.append(page.el);
  const show = (list) => {
    const copy = structuredClone(snap);
    copy.awaiting_approval = { total: list.length, cards: list };
    page.patch(colaView(model(copy), { nowMs, focusKey: list[0].key }));
  };
  show(cards);
  return { page, show };
}

test("a queue row keeps the view it was last given, even when only what the signature does not read changed (the link it opens)", () => {
  const { page, show } = rig([proposal(9), proposal(1)]);
  assert.equal(page.rowView("card:p:1").link, "https://example.com/p/1");
  show([proposal(9), proposal(1, { url: "https://example.com/moved" })]);
  assert.equal(page.rowView("card:p:1").link, "https://example.com/moved", "the view the buttons act on is the one on screen");
});

test("the name of a button is the same on the proposal, on a queue row and after a repaint: an empty company leaves no stray separator", () => {
  const { page, show } = rig([proposal(9, { company: "" }), proposal(1, { company: "" })]);
  const labels = () => [...page.el.querySelectorAll(".acts button[data-action]")].map((b) => b.getAttribute("aria-label"));
  const first = labels();
  assert.ok(first.length >= 2, "the proposal and a row have buttons");
  for (const name of first) assert.doesNotMatch(name, /·\s*Role/, name);
  page.repaint("card:p:9");
  assert.deepEqual(labels(), first, "a repaint says the same names");
  show([proposal(9, { company: "" }), proposal(1, { company: "" })]);
  assert.deepEqual(labels(), first);
});
