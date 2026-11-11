// The acceptance matrix, part 2: the tour (spec A2). The page is walked from the prologue to SISTEMA and back with the keys, at the sizes, themes, hours and
// scenarios of the criterion, on the MOUNTED page with the made-up server of the demo; at every stop nothing may be wrong on screen and nothing may leave the
// machine. Every test of a size and theme is one test, so a failure names the combination (size / theme / scenario / hour / stop).
import test from "node:test";
import assert from "node:assert/strict";
import { openPage, ev, strangers, walk, OPENS } from "./helpers/acceptance.mjs";

const HOURS = [9, 0.43, 18, 22];                       // A2: ?clock= 9, 0,43 (00:26), 18 y 22
const SCENARIOS = ["calm", "urgent", "heavy"];

const sizes = { "1920x1080": [1920, 1080], "3440x1440": [3440, 1440] };
for (const [name, [width, height]] of Object.entries(sizes)) {
  for (const theme of ["dark", "light"]) {
    test(`A2: the tour 0-1-2-3-4-5-0 at ${name} in ${theme === "dark" ? "Tinta" : "Papel"}, hours 9, 00:26, 18 and 22, scenarios calm, urgent and heavy: nothing wrong on screen, console clean, no CSP or Trusted Types violation, nothing leaves 127.0.0.1`, async () => {
      const t = await openPage({ query: `?debug=1&clock=9&lang=es&theme=${theme}`, width, height });
      try {
        const wrong = [];
        for (const scenario of SCENARIOS) for (const hour of HOURS) {
          wrong.push(...await walk(t, `?debug=1&clock=${hour}&lang=es&theme=${theme}&scenario=${scenario}`, `${scenario}@${hour}`));
        }
        assert.deepEqual(wrong, []);
        assert.deepEqual(strangers(t), [], "every request went to the page, its fonts under /a/<id>/ or /api/");
      } finally { await t.close(); }
    });
  }
}

test("A2: the same tour in calm motion (the system's prefers-reduced-motion) at 1920x1080 in Tinta and Papel, hours 9 and 22, urgent and heavy", async () => {
  for (const theme of ["dark", "light"]) {
    const t = await openPage({ query: `?debug=1&clock=9&lang=es&theme=${theme}`, width: 1920, height: 1080, media: { reducedMotion: true } });
    try {
      assert.equal(await ev(t, `document.documentElement.dataset.motion`), "calm");
      const wrong = [];
      for (const scenario of ["urgent", "heavy"]) for (const hour of [9, 22]) wrong.push(...await walk(t, `?debug=1&clock=${hour}&lang=es&theme=${theme}&scenario=${scenario}`, `calm/${theme}/${scenario}@${hour}`));
      assert.deepEqual(wrong, []);
      assert.deepEqual(strangers(t), []);
    } finally { await t.close(); }
  }
});

