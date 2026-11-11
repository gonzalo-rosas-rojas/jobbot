// The acceptance matrix, part 2b: the tour of A2 on the other windows (1440x900, 1366x768, a 390x844 phone) and the words of a missing value with the four scenarios
// (rich, heavy, urgent, empty) in both languages. The big windows are in acceptance.tour.browser.test.mjs; they are two files only because one run of the whole
// matrix of A2 takes more than the six minutes a file may take.
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, ev, run, press, travel, badWordIn, strangers, walk, OPENS, TOUR } from "./helpers/acceptance.mjs";

for (const [name, [width, height]] of Object.entries({ "1440x900": [1440, 900], "1366x768": [1366, 768] })) {
  test(`A2 (laptop): the tour at ${name} in Tinta and Papel, hours 9 and 22, heavy and urgent: the same, with no stop that overflows its window`, async () => {
    for (const theme of ["dark", "light"]) {
      const t = await openPage({ query: `?debug=1&clock=9&lang=es&theme=${theme}`, width, height });
      try {
        const wrong = [];
        for (const scenario of ["heavy", "urgent"]) for (const hour of [9, 22]) wrong.push(...await walk(t, `?debug=1&clock=${hour}&lang=es&theme=${theme}&scenario=${scenario}`, `${name}/${theme}/${scenario}@${hour}`));
        assert.deepEqual(wrong, []);
        assert.deepEqual(strangers(t), []);
      } finally { await t.close(); }
    }
  });
}

test("A2 (phone): the six tabs of a 390x844 phone, in Tinta and Papel, heavy and urgent, hours 9 and 22: the same, on the flat page", async () => {
  for (const theme of ["dark", "light"]) {
    const t = await openPage({ query: `?debug=1&clock=9&lang=es&theme=${theme}`, width: 390, height: 844 });
    try {
      const wrong = [];
      for (const scenario of ["heavy", "urgent"]) for (const hour of [9, 22]) {
        const before = t.page.problems().length;
        await t.page.goto(`${t.url.split("?")[0]}?debug=1&clock=${hour}&lang=es&theme=${theme}&scenario=${scenario}`);
        await t.page.waitFor(OPENS, { timeout: 12000 });
        for (const id of ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"]) {
          await t.page.click(`#tab-${id}`);
          await t.page.waitFor(`document.getElementById("app").dataset.moment === ${JSON.stringify(id)}`);
          const v = await run(t, () => ({ h1: document.querySelector("#app .sheet-host h1")?.textContent.trim() || "", hscroll: document.documentElement.scrollWidth - innerWidth, mode: window.__app.mode() }));
          if (!v.h1 || v.hscroll > 0 || v.mode !== "flat") wrong.push(`${theme}/${scenario}@${hour} ${id}: ${JSON.stringify(v)}`);
          const bad = await badWordIn(t);
          if (bad) wrong.push(`${theme}/${scenario}@${hour} ${id}: "${bad}"`);
        }
        wrong.push(...t.page.problems().slice(before).map((p) => `${theme}/${scenario}@${hour}: ${p.text.slice(0, 160)}`));
      }
      assert.deepEqual(wrong, []);
      assert.deepEqual(strangers(t), []);
    } finally { await t.close(); }
  }
});

// ---- the words of a missing value, with the four scenarios (rich, heavy, urgent and empty) in both languages ----

test("A2 (words): no 'null', 'undefined', 'NaN', '[object' or ⟦missing key⟧ is readable at any stop with the scenarios rich, heavy, urgent and empty, in Spanish and English", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    const wrong = [];
    for (const scenario of ["", "heavy", "urgent", "empty"]) for (const lang of ["es", "en"]) {
      const before = t.page.problems().length;
      await t.page.goto(`${t.url.split("?")[0]}?debug=1&clock=9&lang=${lang}${scenario ? "&scenario=" + scenario : ""}`);
      await t.page.waitFor(OPENS, { timeout: 12000 });
      for (const id of TOUR) {
        await travel(t, id, 8000);
        await t.page.wait(120);
        const bad = await badWordIn(t);
        if (bad) wrong.push(`${scenario || "rich"}/${lang}/${id}: "${bad}"`);
      }
      // the drawer and the help are stops of their own: an application open, and the sheet of keys
      await travel(t, "procesos");
      if (scenario !== "empty") {
        await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 0`);
        await press(t, "j");
        await press(t, "Enter");
        await t.page.waitFor(`document.getElementById("detail").classList.contains("open") && !!document.querySelector("#detail .d-body, #detail [data-job]")`, { timeout: 6000 });
        await t.page.wait(250);
        const bad = await badWordIn(t);
        if (bad) wrong.push(`${scenario || "rich"}/${lang}/drawer: "${bad}"`);
        await press(t, "Escape");
        await t.page.waitFor(`!document.getElementById("detail").classList.contains("open")`);
      }
      await press(t, "?", { code: "Slash", modifiers: 8 });
      await t.page.waitFor(`!!document.querySelector(".help")`);
      const bad = await badWordIn(t);
      if (bad) wrong.push(`${scenario || "rich"}/${lang}/help: "${bad}"`);
      await press(t, "Escape");
      wrong.push(...t.page.problems().slice(before).map((p) => `${scenario || "rich"}/${lang}: ${p.text.slice(0, 160)}`));
    }
    assert.deepEqual(wrong, []);
  } finally { await t.close(); }
});

test("A2 (words): the check itself is not vacuous: a page that prints 'undefined' or [object Object] is caught", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    assert.equal(await badWordIn(t), null);
    for (const word of ["undefined", "null", "NaN", "[object Object]", "⟦hoy.title⟧"]) {
      await ev(t, `window.__probe = document.createElement("p"); __probe.textContent = ${JSON.stringify("hay " + word + " aquí")}; document.getElementById("app").append(__probe)`);
      assert.ok(await badWordIn(t), `"${word}" on screen is caught`);
      await ev(t, `__probe.remove()`);
    }
    await ev(t, `window.__probe = document.createElement("p"); __probe.setAttribute("aria-label", "total NaN"); document.getElementById("app").append(__probe)`);
    assert.ok(await badWordIn(t), "and in an aria-label");
    await ev(t, `__probe.remove()`);
    for (const fine of ["Nulo", "nulidad", "Banana", "Annan"]) {
      await ev(t, `window.__probe = document.createElement("p"); __probe.textContent = ${JSON.stringify(fine)}; document.getElementById("app").append(__probe)`);
      assert.equal(await badWordIn(t), null, `"${fine}" is a word`);
      await ev(t, `__probe.remove()`);
    }
  } finally { await t.close(); }
});
