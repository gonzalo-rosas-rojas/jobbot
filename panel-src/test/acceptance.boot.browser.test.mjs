// The acceptance matrix, part 1: the page as it ARRIVES (spec A1, A4, A5, A6, A8, A9, A11, A12). Every test is named by its criterion and runs against the
// mounted page (the demo build with the made-up server, or the same page with the server cut out and the test answering /api/* from the fixtures) or
// against the files the page is made of. The criteria that need a whole tour are in acceptance.tour.*, the keys and the camera in acceptance.nav.*,
// the geometry and the light in acceptance.look.*, the contrast sweep in acceptance.contrast.*. docs/research/2026-10-07-revision/ACEPTACION-A1-A19.md is the table.
import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { openPage, ev, run, press, travel, STOPS, badWordIn, readable, strangers } from "./helpers/acceptance.mjs";
import { guard, HTML_GZ_BUDGET, JS_BUDGET, FONT_FILES } from "../guard.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const repo = path.join(root, "..");
const fixture = (name) => JSON.parse(readFileSync(path.join(root, "test", "fixtures", `snapshot.${name}.json`), "utf8"));
const answerWith = (snapshot, calls = []) => async (p) => { calls.push(p); return p.startsWith("/api/snapshot") ? { status: 200, body: snapshot } : undefined; };

// ---- A1 -----------------------------------------------------------------------------------------------------------------------------------

test("A1: opening /?t= with data shows HOY with data less than 1 s after the document arrives, and the tab says Panel before, during and after", async () => {
  const snapshot = fixture("rich");
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  // the snapshot is held for 300 ms, so the page has to be painted BEFORE it: the HTML paints the scenario, the data fills it
  const answer = async (p) => { if (!p.startsWith("/api/snapshot")) return undefined; await gate; return { status: 200, body: snapshot }; };
  const t = await openPage({ kind: "real", query: "?t=tok-A1&debug=1&clock=9&lang=es", width: 1920, height: 1080, answer, wait: false });
  try {
    await t.page.waitFor(`document.querySelectorAll('#stops [role="tab"]').length === 5`);
    const before = await ev(t, `({ title: document.title, h1: document.querySelector("#app .sheet-host h1")?.textContent || "", skeletonAt: Math.round(performance.now() - performance.getEntriesByType("navigation")[0].responseEnd), sky: document.getElementById("sky").width > 0 })`);
    assert.equal(before.title, "Panel", "the title before the data");
    // and the document as it is served says it before any script has run (the script of the page also pins it, so only the markup shows a wrong one)
    const served = await ev(t, `fetch(${JSON.stringify(t.url.split("#")[0])}).then((r) => r.text())`);
    assert.equal((/<title>([^<]*)<\/title>/.exec(served) || [])[1], "Panel", "the <title> of the served document");
    assert.ok(!/te tocan\s+\S+/i.test(before.h1) || before.h1 === "", `before the snapshot the heading is a skeleton, not data: "${before.h1}"`);
    assert.ok(before.skeletonAt < 1000 && before.sky, `the stage, the stops and the sheet exist ${before.skeletonAt} ms after the document, with no data yet`);
    await t.page.wait(300);
    release();
    await t.page.waitFor(`/te tocan/i.test(document.querySelector("#app .sheet-host h1")?.textContent || "")`, { timeout: 3000 });
    // an upper bound: the poll of waitFor adds up to 50 ms and the answer travels through the test (Fetch interception)
    const dataAt = await ev(t, `Math.round(performance.now() - performance.getEntriesByType("navigation")[0].responseEnd)`);
    assert.ok(dataAt - 300 < 1000, `HOY with data ${dataAt - 300} ms after the document arrived (the 300 ms the test held the snapshot back are not counted)`);
    assert.equal(await ev(t, `document.title`), "Panel", "the title with the data");
    assert.equal(await ev(t, `new URL(location.href).searchParams.has("t")`), false, "the token left the address");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// The demo names itself (forja, docs/research/2026-11-06-panel-obra-de-arte.md, PO-09): its tab says jobbot. What A1 guards is the same either way: the tab
// never changes with the stop, the language, the drawer or the width, so it never says what is on screen. The refused token and page.html are the real page.
const DEMO_TITLE = "jobbot";

test("A1: the title stays the same on every stop, in both languages, with the drawer open and on the phone (jobbot in the demo), and is Panel after a refused token", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1440, height: 900 });
  try {
    for (const id of ["prologo", "hoy", "procesos", "cola", "aprender", "sistema"]) {
      await travel(t, id);
      assert.equal(await ev(t, `document.title`), DEMO_TITLE, `${id} in es`);
    }
    await press(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "en"`);
    for (const id of ["hoy", "cola", "sistema"]) { await travel(t, id); assert.equal(await ev(t, `document.title`), DEMO_TITLE, `${id} in en`); }
    await travel(t, "procesos");
    await t.page.waitFor(`document.querySelectorAll("#app [data-card]").length > 0`);
    await press(t, "j");
    await press(t, "Enter");
    await t.page.waitFor(`document.getElementById("detail").classList.contains("open")`);
    assert.equal(await ev(t, `document.title`), DEMO_TITLE, "with an application open in the drawer");
    await t.page.viewport(390, 844);
    await t.page.waitFor(`window.__app.mode() === "flat"`);
    assert.equal(await ev(t, `document.title`), DEMO_TITLE, "on the phone");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
  const refused = await openPage({ kind: "real", query: "?t=wrong&debug=1&clock=9", width: 1440, height: 900, answer: async () => ({ status: 401, body: { message: "No autorizado" } }), wait: false });
  try {
    await refused.page.waitFor(`!!document.querySelector(".sheet-banner") && !document.querySelector(".sheet-banner").hidden`);
    assert.equal(await ev(refused, `document.title`), "Panel", "after a refused token");
  } finally { await refused.close(); }
  const source = readFileSync(path.join(repo, "crates", "jobbot-panel", "src", "page.html"), "utf8");
  assert.match(source, /<title>Panel<\/title>/, "the <title> of the page Rust serves");
});

// ---- A4 -----------------------------------------------------------------------------------------------------------------------------------

const sha256 = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

test("A4: the page asks for its five fonts under /a/<launch id>/, each answers 200 as font/woff2 with the year-long immutable cache; another name or another id is 404", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    await t.page.waitFor(`document.fonts.status === "loaded"`);
    const seen = await run(t, () => performance.getEntriesByType("resource").filter((r) => /\/a\/[0-9a-f]{16}\//.test(r.name)).map((r) => ({ url: r.name, status: r.responseStatus })));
    const names = seen.map((s) => s.url.split("/").pop()).sort();
    assert.deepEqual(names.filter((n) => n.endsWith(".woff2")), [...FONT_FILES].sort(), "the five fonts of the page, no more");
    for (const s of seen) { assert.ok(s.url.includes(`/a/${t.launchId}/`), s.url); assert.equal(s.status, 200, s.url); }
    const faces = await run(t, () => [...document.fonts].map((f) => `${f.family}|${f.weight}|${f.style}|${f.status}`).sort());
    assert.equal(faces.length, 5);
    for (const f of faces) assert.ok(f.endsWith("|loaded"), `${f} is loaded`);
    const probe = (p) => run(t, async (path) => { const r = await fetch(path); return { status: r.status, type: r.headers.get("content-type"), cache: r.headers.get("cache-control") }; }, p);
    const ok = await probe(`/a/${t.launchId}/barlow-condensed-700.woff2`);
    assert.deepEqual(ok, { status: 200, type: "font/woff2", cache: "public, max-age=31536000, immutable" });
    assert.equal((await probe(`/a/${t.launchId}/not-a-font.woff2`)).status, 404, "another name");
    assert.equal((await probe(`/a/0000000000000000/barlow-condensed-700.woff2`)).status, 404, "another launch id");
    assert.deepEqual(t.page.problems().filter((p) => !/404/.test(p.text)), [], "(the two 404 above are the point)");
  } finally { await t.close(); }
});

test("A4: every asset the panel ships has its row in THIRD_PARTY_NOTICES.md with its type and the sha256 of the file itself, and no row lacks its file", () => {
  const notices = readFileSync(path.join(repo, "THIRD_PARTY_NOTICES.md"), "utf8").split(/\r?\n/).filter((l) => l.startsWith("| `"));
  const dirs = [path.join(repo, "crates", "jobbot-panel", "src", "assets"), path.join(root, "assets", "fonts")];
  const files = dirs.flatMap((d) => (existsSync(d) ? readdirSync(d).filter((n) => /\.(woff2|svg|png)$/.test(n)).map((n) => path.join(d, n)) : []));
  assert.ok(files.length >= 5, `at least the five fonts (${files.length})`);
  for (const file of files) {
    const name = path.basename(file);
    const row = notices.find((r) => r.startsWith(`| \`${name}\` |`));
    assert.ok(row, `${name} has no row in THIRD_PARTY_NOTICES.md`);
    assert.ok(row.includes(name.endsWith(".woff2") ? "font/woff2" : name.endsWith(".svg") ? "image/svg+xml" : "image/png"), `${name}: the row names the type`);
    const hash = row.split("`").reverse()[1];
    assert.equal(hash, sha256(file), `${name}: the sha256 of the row is not the sha256 of the file`);
  }
  const names = new Set(files.map((f) => path.basename(f)));
  for (const row of notices.filter((r) => /font\/|image\//.test(r))) {
    const name = row.slice(3, row.indexOf("`", 3));
    assert.ok(names.has(name), `${name} has a row but no file`);
  }
});

// ---- A5 -----------------------------------------------------------------------------------------------------------------------------------

const CSP_SPEC = ["default-src 'none'", "script-src 'nonce-X'", "style-src 'nonce-X'", "connect-src 'self'", "img-src 'self' blob:", "font-src 'self'", "base-uri 'none'", "form-action 'none'", "frame-ancestors 'none'", "require-trusted-types-for 'script'"];

test("A5: the CSP of / is exactly the one of spec 9.6 (the page's and Rust's), no script or style lacks the nonce, and every url( is an own asset or a fragment", async () => {
  // Rust's: the format string in crates/jobbot-panel/src/http.rs, joined (the Rust string breaks its lines with a backslash)
  const rs = readFileSync(path.join(repo, "crates", "jobbot-panel", "src", "http.rs"), "utf8");
  const at = rs.indexOf("\"default-src 'none'; script-src 'nonce-{nonce}'");
  assert.ok(at > 0, "the CSP of Rust is in crates/jobbot-panel/src/http.rs");
  const literal = rs.slice(at + 1, rs.indexOf("\"", at + 1)).replace(/\\\r?\n\s*/g, "");
  assert.equal(literal.replaceAll("{nonce}", "X"), CSP_SPEC.join("; "), "Rust's header is the spec's, directive by directive");
  const t = await openPage({ query: "?debug=1&clock=9", width: 1440, height: 900 });
  try {
    // the header of the very response that the page was made from: the document is loaded again with a listener on
    let header = null;
    t.page.ws.addEventListener("message", (event) => {
      const msg = JSON.parse(event.data);
      if (msg.method === "Network.responseReceived" && msg.params.type === "Document") header = msg.params.response.headers["content-security-policy"] || msg.params.response.headers["Content-Security-Policy"];
    });
    await t.page.goto(t.url);
    await t.page.waitFor(`!!document.querySelector("#app .sheet-host h1")`);
    assert.ok(header, "the document came with a Content-Security-Policy");
    const v = await run(t, () => {
      const tags = [...document.querySelectorAll("script, style")].map((el) => ({ tag: el.tagName, nonce: el.nonce }));
      const css = [...document.styleSheets].flatMap((s) => [...s.cssRules].map((r) => r.cssText)).join("\n");
      return { tags, urls: [...css.matchAll(/url\(\s*["']?([^"')]+)/g)].map((m) => m[1]) };
    });
    v.header = header;
    v.nonce = (/'nonce-([0-9a-f]+)'/.exec(header) || [])[1];
    assert.ok(v.nonce, "the header carries a nonce");
    assert.equal(v.header.replaceAll(v.nonce, "X"), CSP_SPEC.join("; "), "the header the mounted page was served with");
    assert.ok(v.tags.length >= 3, "two scripts and a style");
    for (const tag of v.tags) assert.equal(tag.nonce, v.nonce, `${tag.tag} carries the nonce of the header`);
    assert.ok(v.urls.length >= 5, `the page names its fonts with url() (${v.urls.length})`);
    for (const u of v.urls) assert.ok(new RegExp(`^(/a/${t.launchId}/[a-z0-9._-]+|#[\\w-]+)$`).test(u), `url(${u}) is neither an own asset nor a fragment`);
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
  const page = readFileSync(path.join(repo, "crates", "jobbot-panel", "src", "page.html"), "utf8");
  assert.equal(page.split("<script").length - 1, page.split('<script nonce="__NONCE__"').length - 1, "page.html: every script has the nonce");
  assert.equal(page.split("<style").length - 1, page.split('<style nonce="__NONCE__"').length - 1, "page.html: every style has the nonce");
  assert.doesNotMatch(page, /url\(\s*["']?(?:https?:|data:|\/\/)/i, "page.html: no url( to the outside");
});

test("A5: the CSP and Trusted Types are really on, and problems() would have seen a breach (the measure is not vacuous)", async () => {
  const t = await openPage({ query: "?debug=1&clock=9", width: 1440, height: 900 });
  try {
    assert.deepEqual(t.page.problems(), [], "a clean page first");
    // (a script run through the DevTools protocol is exempt from the CSP, so eval cannot be the control: markup and an image can)
    const outcome = await run(t, () => {
      const out = {};
      try { document.body.innerHTML = "<b>x</b>"; out.markup = "accepted"; } catch (e) { out.markup = e.name; }
      const img = document.createElement("img");
      img.src = "http://203.0.113.9/outside.png";
      document.body.append(img);
      return out;
    });
    assert.equal(outcome.markup, "TypeError", "a string given to innerHTML is refused (require-trusted-types-for 'script')");
    await t.page.wait(300);
    const seen = t.page.problems().map((p) => p.text).join("\n");
    assert.match(seen, /CSP violation: require-trusted-types-for/, "problems() sees the Trusted Types breach");
    assert.match(seen, /CSP violation: img-src/, "and a request to an outside address (the CSP refuses it before it leaves)");
    assert.deepEqual(strangers(t), ["http://203.0.113.9/outside.png"], "and the request log of the tour would show it (the measure of 'no request outside 127.0.0.1')");
  } finally { await t.close(); }
});

// ---- A6 -----------------------------------------------------------------------------------------------------------------------------------

test("A6: the whole page, built under the strict guard, is within the budgets (HTML gzip, JS, fonts), has no data: and no node the story has not asked for", () => {
  const out = path.join(root, "dist", `acc-a6-${process.pid}.html`);
  mkdirSync(path.dirname(out), { recursive: true });
  try {
    const log = execFileSync(process.execPath, ["build.mjs", "--out", path.relative(root, out)], { cwd: root, encoding: "utf8" });
    assert.match(log, /^page [\d.]+ KB \(script ([\d.]+) KB/, "the build says what it weighed");
    const html = readFileSync(out, "utf8");
    const gz = gzipSync(html).length;
    const script = Number(/script ([\d.]+) KB/.exec(log)[1]);
    assert.ok(gz <= HTML_GZ_BUDGET, `the page is ${(gz / 1024).toFixed(1)} KB gzipped, over the ${HTML_GZ_BUDGET / 1024} KB cap (D-20)`);
    assert.ok(script * 1024 <= JS_BUDGET, `the script is ${script} KB, over ${JS_BUDGET / 1024} KB`);
    const fonts = FONT_FILES.reduce((n, f) => n + statSync(path.join(repo, "crates", "jobbot-panel", "src", "assets", f)).size, 0);
    assert.ok(fonts <= 120 * 1024, `the five fonts weigh ${(fonts / 1024).toFixed(1)} KB, over 120 KB`);
    assert.doesNotMatch(html, /url\(\s*["']?data:|src=["']data:|href=["']data:/i, "no data: in the page");
    // the same page the Rust binary embeds, held to the same caps
    const embedded = readFileSync(path.join(repo, "crates", "jobbot-panel", "src", "page.html"), "utf8");
    assert.ok(gzipSync(embedded).length <= HTML_GZ_BUDGET, "crates/jobbot-panel/src/page.html is within the gzip cap");
    assert.doesNotMatch(embedded, /url\(\s*["']?data:|src=["']data:/i, "crates/jobbot-panel/src/page.html has no data:");
    assert.deepEqual(guard(embedded, embedded.split(/<script[^>]*>/)[2]?.split("</script>")[0] || "", { strictPage: true, budget: JS_BUDGET * 2 }).filter((p) => !/budget/.test(p)), [], "crates/jobbot-panel/src/page.html passes the strict guard");
  } finally { rmSync(out, { force: true }); }
});

test("A6: in work mode the page mounts the sky of one stage and nothing of a story: no relato node, no canvas outside the stage, one stage", async () => {
  const t = await openPage({ query: "?debug=1&clock=9&relato=1", width: 1920, height: 1080 });
  try {
    await t.page.wait(600);
    // The sky is one stage painted in two layers: #sky (gradient, stars, light and the far skyline) and .sky-front (the near buildings, over the dial, so that the
    // dial of HOY and SISTEMA can sink behind the city). Both are made and removed by the same stage; what A6 forbids is a canvas of the relato or of anything else.
    const v = await run(t, () => ({ canvases: [...document.querySelectorAll("canvas")].map((c) => (c.closest("#stage") ? c.id || c.className : "outside")).sort(), nodes: document.querySelectorAll("*").length, stage: document.querySelectorAll("#stage").length, acts: document.querySelectorAll('[class*="relato"], [id*="relato"], [data-relato], [class^="acto"], [id^="acto"], [data-act]').length }));
    assert.deepEqual(v.canvases, ["sky", "sky-front"], "the only canvases are the two layers of the stage's sky");
    assert.equal(v.stage, 1);
    assert.equal(v.acts, 0, "no act of the relato is in the DOM");
    assert.ok(v.nodes < 1500, `${v.nodes} nodes in work mode`);
  } finally { await t.close(); }
});

test("A6: the relato (the eight acts) mounts nothing in work mode, and only when asked", { todo: "the relato is not built yet (F3): there is no ?relato=1, no acts and no '0 twice' to demonstrate that it mounts lazily; today the clause holds vacuously" }, async () => {
  assert.fail("no relato to ask for");
});

// ---- A8 -----------------------------------------------------------------------------------------------------------------------------------

test("A8: es and en have the same keys and the same {variables}, and the labels of the dial on the mounted page follow the language", async () => {
  const SCOPES = ["common", "shell", "hoy", "procesos", "cola", "aprender", "sistema", "detail", "stage", "hud", "keys", "moments"];
  for (const scope of SCOPES) {
    const [es, en] = await Promise.all(["es", "en"].map(async (l) => (await import(`../src/i18n/${l}/${scope}.js`)).default));
    assert.deepEqual(Object.keys(es).sort(), Object.keys(en).sort(), `${scope}: the same keys`);
    for (const k of Object.keys(es)) {
      const vars = (s) => [...new Set([...String(s).matchAll(/\{([A-Za-z_]\w*)\}/g)].map((m) => m[1]))].sort();
      assert.deepEqual(vars(es[k]), vars(en[k]), `${scope}.${k}: the same variables`);
    }
  }
  const t = await openPage({ query: "?debug=1&clock=9&lang=es", width: 1920, height: 1080 });
  try {
    const labels = () => run(t, () => [...document.querySelectorAll("#dial text")].map((e) => e.textContent.replace(/\s+/g, " ").trim()).filter((s) => /\p{L}/u.test(s)));
    const words = async (l) => { const d = (await import(`../src/i18n/${l}/stage.js`)).default; return [d["dial.window"], d["dial.local"]].map((s) => s.replace(/\s+/g, " ").trim()); };
    assert.deepEqual(await labels(), await words("es"), "the labels of the dial are the dictionary's, in Spanish");
    await press(t, "l");
    await t.page.waitFor(`document.documentElement.lang === "en"`);
    assert.deepEqual(await labels(), await words("en"), "and in English, after l");
    assert.notDeepEqual(await words("es"), await words("en"));
    assert.equal(await ev(t, `document.querySelector("#dial svg").getAttribute("aria-label")`), (await import("../src/i18n/en/stage.js")).default["dial.aria"], "and the aria-label of the dial");
    assert.equal(await badWordIn(t), null, "no ⟦missing key⟧ on screen");
    assert.deepEqual(t.page.problems(), []);
  } finally { await t.close(); }
});

// ---- A9 (the page's side) -----------------------------------------------------------------------------------------------------------------

test("A9: with the snapshot of the tests no full e-mail address, no Windows user folder and no drive path is anywhere a person can read it, on any stop, in either language", async () => {
  const FULL_ADDRESS = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+/;
  const USER_FOLDER = /[A-Za-z]:\\|\/Users\/|\\Users\\|\/home\/[a-z]/i;
  for (const name of ["rich", "heavy"]) {
    const t = await openPage({ kind: "real", query: "?t=tok&debug=1&clock=9&lang=es", width: 1440, height: 900, answer: answerWith(fixture(name)) });
    try {
      for (const lang of ["es", "en"]) {
        if (lang === "en") { await press(t, "l"); await t.page.waitFor(`document.documentElement.lang === "en"`); }
        for (const id of STOPS) {
          await travel(t, id);
          await t.page.wait(150);
          const words = await readable(t);
          const address = FULL_ADDRESS.exec(words), folder = USER_FOLDER.exec(words);
          assert.equal(address && address[0].replace(/^(.{2}).*(@.*)$/, "$1***$2"), null, `${name}/${lang}/${id}: a full e-mail address is on screen`);
          assert.equal(folder && folder[0], null, `${name}/${lang}/${id}: a path of the user's folder is on screen`);
        }
      }
      const html = await ev(t, `document.documentElement.outerHTML`);
      assert.doesNotMatch(html, FULL_ADDRESS, `${name}: nor in the markup (data-*, hrefs, titles)`);
      // the test answers the snapshot only: the detail and the capture of a card are 404 for it, and Chrome logs each one
      assert.deepEqual(t.page.problems().filter((p) => !/status of 404.*\/api\/(job|shot)\?key=/.test(p.text)), []);
    } finally { await t.close(); }
  }
});

// ---- A11 ----------------------------------------------------------------------------------------------------------------------------------

test("A11: node build.mjs (the strict guard) passes on the page, and refuses a bundle that breaks it (innerHTML) without writing a page", () => {
  const entry = path.join(root, "dist", `acc-a11-entry-${process.pid}.js`), out = path.join(root, "dist", `acc-a11-${process.pid}.html`);
  mkdirSync(path.join(root, "dist"), { recursive: true });
  try {
    const good = spawnSync(process.execPath, ["build.mjs", "--out", path.relative(root, out)], { cwd: root, encoding: "utf8" });
    assert.equal(good.status, 0, `the page does not pass the guard:\n${good.stderr}`);
    rmSync(out, { force: true });
    writeFileSync(entry, 'document.body.innerHTML = "<b>x</b>";');
    const bad = spawnSync(process.execPath, ["build.mjs", "--entry", path.relative(root, entry), "--out", path.relative(root, out)], { cwd: root, encoding: "utf8" });
    assert.notEqual(bad.status, 0, "a page with innerHTML must be refused");
    assert.match(bad.stderr, /innerHTML/);
    assert.equal(existsSync(out), false, "and nothing is written");
  } finally { rmSync(entry, { force: true }); rmSync(out, { force: true }); }
});

// ---- A12 ----------------------------------------------------------------------------------------------------------------------------------

test("A12: its own tests run (test/demo.contract.test.mjs), and the built demo page holds only invented addresses, hosts and no Windows path, phone or database", () => {
  // 1. what A12 already has: run it and read its verdict (the third test is skipped where scripts/demo/private-strings.local.txt does not exist)
  const env = { ...process.env }; delete env.NODE_TEST_CONTEXT;               // a test that runs a test run: without this the child reports to its parent
  const own = spawnSync(process.execPath, ["--test", "--test-reporter=spec", "test/demo.contract.test.mjs"], { cwd: root, encoding: "utf8", env });
  assert.equal(own.status, 0, `test/demo.contract.test.mjs fails:\n${own.stdout.slice(-1500)}`);
  const a12 = [...own.stdout.matchAll(/^[✔✖﹣] (A12:[^\r\n]*)/gmu)].map((m) => m[1]);
  assert.equal(a12.length, 3, `the three A12 tests are there: ${a12.join(" | ")}`);
  if (/NOT CHECKED against private strings/.test(own.stdout)) console.warn("A12: the local list of private strings does not exist on this machine, so only the paths were checked (see the report)");
  // 2. the demo page as built: nothing of the person behind it. The addresses are invented (.example or example.com), the hosts are two, and no folder of a user or phone number is in it.
  // The one exception is the pair of public links of the demo's line about itself (scripts/demo/demo.js, `about`, drawn by HOY): the code and the
  // showcase of jobbot, which Gonza decided to name (forja, docs/research/2026-11-06-panel-obra-de-arte.md, PO-09). They are taken out whole before the
  // hosts are read, so any other address of github.com or github.io still fails, and they must be there.
  const out = path.join(root, "dist", `acc-a12-${process.pid}.html`);
  try {
    execFileSync(process.execPath, ["build.mjs", "demo", "--out", path.relative(root, out)], { cwd: root, stdio: "pipe" });
    const html = readFileSync(out, "utf8");
    const addresses = [...new Set(html.match(/[A-Za-z0-9._%+*-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+/g) || [])];
    assert.ok(addresses.length >= 3, "the demo has mail to show");
    for (const a of addresses) assert.match(a, /@([a-z0-9-]+\.example|example\.(com|org|net))$/, `a real-looking address in the demo page: ${a.replace(/^(.).*@/, "$1***@")}`);
    const PUBLIC = ["https://github.com/gonzalo-rosas-rojas/jobbot", "https://gonzalo-rosas-rojas.github.io/jobbot-showcase/"];
    for (const url of PUBLIC) assert.ok(html.includes(url), `the demo links to ${url}`);
    const rest = PUBLIC.reduce((text, url) => text.split(url).join(""), html);
    const hosts = [...new Set((rest.match(/https?:\/\/[A-Za-z0-9.-]+/g) || []).map((u) => u.replace(/^https?:\/\//, "")))].filter((h) => !/(^|\.)example\.(com|org|net)$|\.example$|^www\.w3\.org$/.test(h));
    assert.deepEqual(hosts, [], "every address of the demo page is invented, or the SVG namespace");
    assert.doesNotMatch(html, /[A-Za-z]:\\\\Users|\/Users\/[A-Za-z]|\+\d[\d ()-]{9,}\d|\.sqlite|results\.jsonl|handoff\//i, "no user folder, phone number, database or hand-off file");
  } finally { rmSync(out, { force: true }); }
});

// A12 asks the demo for "no string of profile/, data/ or cv/". What it protects is the person behind the demo: a folder of his, a file of his, his name, his mail.
// The page also has sentences that tell him which file OF THE PROJECT he would edit (aprender.how, sistema.account.unanchored.why, sistema.spend.nobudget,
// sistema.limits.ro): those are the repository's own documentation, the same on every install, and the coordinator decided (2026-10-09, reversible) that they stay.
// So the check is a short, explicit and commented list of RELATIVE, NEUTRAL project paths, compared against the WHOLE token that holds the folder name (the run of
// path characters around it), never against the folder name alone. A relative path of the project is allowed only if it is exactly one of these four; an absolute path
// (a drive, a root, a home), a Windows user folder, a path glued to an address, any other file of those folders, a data/ or a cv/ path: still forbidden.
const PROJECT_PATHS = new Set([
  "profile/answers.toml",       // the answer bank he fills in by hand (APRENDER: how to add an answer)
  "profile/claude.local.toml",  // the local file of the account guard (SISTEMA: unanchored account)
  "profile/claude.toml",        // the file that holds the spending limit (SISTEMA: spend without a limit)
  "profile/*.toml",             // the same three as a pattern (SISTEMA: limits are read only)
]);

/** The tokens of `text` that hold a profile/, data/ or cv/ folder as a whole path segment and are NOT one of PROJECT_PATHS, whole. */
function foreignFolderPaths(text, allowed = PROJECT_PATHS) {
  const tokens = text.match(/[\w.*:~%$@\\/-]+/g) || [];
  return [...new Set(tokens.filter((tok) => /(^|[\\/:@])(profile|data|cv)\//i.test(tok) && !allowed.has(tok.replace(/\.+$/, ""))))];   // a full stop that closes a sentence is not part of the path
}

test("A12: the built demo page holds no path of profile/, data/ or cv/ but the four neutral project paths of PROJECT_PATHS, and no absolute path or address beside them", () => {
  const out = path.join(root, "dist", `acc-a12b-${process.pid}.html`);
  try {
    execFileSync(process.execPath, ["build.mjs", "demo", "--out", path.relative(root, out)], { cwd: root, stdio: "pipe" });
    const html = readFileSync(out, "utf8");
    assert.deepEqual(foreignFolderPaths(html), [], "paths of the private folders in the page that is published as the demo, beyond the four project paths");
    // the allowed ones really are there (if the sentences go, this list should shrink with them: an allowance nobody uses is a hole waiting)
    for (const p of PROJECT_PATHS) assert.ok(html.includes(p), `${p} is in the demo page (otherwise take it out of PROJECT_PATHS)`);
  } finally { rmSync(out, { force: true }); }
});

test("A12: the list of project paths is not a hole: an absolute path, a Windows user folder, an address, another file or a data/ or cv/ path still counts as foreign", () => {
  const ok = "Add an entry to profile/answers.toml, then profile/claude.local.toml, profile/claude.toml and profile/*.toml.";
  assert.deepEqual(foreignFolderPaths(ok), [], "the four project paths, said as the page says them, pass");
  const bad = [
    "C:\\Users\\ana\\profile/answers.toml",            // a Windows user folder around an allowed path
    "C:/Users/ana/profile/claude.toml",                      // a drive
    "/home/ana/profile/claude.local.toml",                   // a root
    "~/profile/answers.toml",                                // a home
    "%LOCALAPPDATA%/profile/answers.toml",                   // an environment folder
    "ana@mail.example/profile/claude.toml",                  // glued to an address
    "profile/answers.toml@mail.example",                     // an address built on an allowed path
    "profile/answers.toml.bak",                              // another file that starts like an allowed one
    "profile/criteria.toml",                                 // another file of the folder
    "profile/ana-garcia/cv.toml",                            // a name inside the folder
    "cv/ana-garcia.pdf",                                     // the cv folder
    "data/results.sqlite",                                   // the data folder
    "https://host.example/data/panel.json",                  // a data/ path of an address
  ];
  for (const one of bad) assert.deepEqual(foreignFolderPaths(`see ${one} here`), [one], `still foreign: ${one}`);
  assert.deepEqual(foreignFolderPaths("the metadata/ folder, a candata/x and a cv-profile/x"), [], "a name that merely ends like a folder is not one (no false alarm)");
  assert.deepEqual(foreignFolderPaths("see profile/answers.toml", new Set()), ["profile/answers.toml"], "with an empty list the allowed path is foreign again: the list is what lets it through");
});
