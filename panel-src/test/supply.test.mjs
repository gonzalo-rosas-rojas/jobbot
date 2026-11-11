// check-lock.mjs is the gate between the registry and the page, so each rule gets a test that bends one input and
// expects the gate to name it. The real files must pass untouched; nothing here needs the network.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { checkOffline, checkRegistry, licenseAllowed, EXPECTED, REQUIRED_NPMRC } from "../check-lock.mjs";
import { buildNotices, NOTICES } from "../tools/notices.mjs";

const read = (f) => readFileSync(new URL("../" + f, import.meta.url), "utf8");
const real = () => ({
  pkg: JSON.parse(read("package.json")),
  lock: JSON.parse(read("package-lock.json")),
  notices: read("THIRD_PARTY_NOTICES.md"),
  npmrc: read(".npmrc"),
  licenseOf: (name) => {
    try { return JSON.parse(read(`node_modules/${name}/package.json`)).license; } catch { return undefined; }
  },
});
const failures = (input, expected) => checkOffline({ ...input, ...(expected ? { expected } : {}) }).filter((r) => !r.ok);
const names = (rows) => rows.map((r) => `${r.id}: ${r.note}`).join("\n");

test("the real project passes every offline check", () => {
  assert.deepEqual(failures(real()), []);
});

test("a range in package.json is refused", () => {
  const input = real();
  input.pkg.dependencies.lenis = "^1.3.26";
  assert.match(names(failures(input)), /package\.json lenis: "\^1\.3\.26" is not an exact version/);
});

test("a package that is declared but not approved is refused", () => {
  const input = real();
  input.pkg.dependencies["left-pad"] = "1.3.0";
  assert.match(names(failures(input)), /package\.json left-pad: declared but not approved/);
});

test("a changed tarball hash in the lock is refused", () => {
  const input = real();
  input.lock.packages["node_modules/tinykeys"].integrity = "sha512-" + "A".repeat(86) + "==";
  assert.match(names(failures(input)), /lock tinykeys: .*differs from EXPECTED/);
});

test("a package nobody approved in the lock is refused, an esbuild platform binary is not", () => {
  const input = real();
  input.lock.packages["node_modules/evil-dep"] = { version: "1.0.0", integrity: "sha512-x", resolved: "https://registry.npmjs.org/evil-dep/-/evil-dep-1.0.0.tgz", license: "MIT" };
  assert.match(names(failures(input)), /lock evil-dep: a package nobody approved/);
});

test("a package resolved from outside the registry is refused", () => {
  const input = real();
  input.lock.packages["node_modules/lenis"].resolved = "https://example.invalid/lenis-1.3.26.tgz";
  assert.match(names(failures(input)), /lock lenis: resolves from https:\/\/example\.invalid/);
});

test("an install script in any package but esbuild is refused", () => {
  const input = real();
  input.lock.packages["node_modules/@leeoniya/ufuzzy"].hasInstallScript = true;
  assert.match(names(failures(input)), /install scripts: .*ufuzzy/);
});

test("a licence outside the allow-list is refused, read from the installed package", () => {
  const input = real();
  input.licenseOf = (name) => (name === "lenis" ? "GPL-3.0" : "MIT");
  assert.match(names(failures(input)), /licence lenis: GPL-3\.0/);
  assert.ok(licenseAllowed("MIT") && licenseAllowed("(MIT OR ISC)") && licenseAllowed("OFL-1.1"));
  assert.ok(!licenseAllowed("GPL-3.0") && !licenseAllowed("(MIT AND Prosperity-3.0.0)") && !licenseAllowed(undefined));
});

test("a version younger than 7 days when adopted is refused", () => {
  const expected = structuredClone(EXPECTED);
  expected["lucide-static"].published = "2026-10-08"; // the day 1.53.0 came out; adopted the same day
  const rows = failures(real(), expected);
  assert.match(names(rows), /quarantine lucide-static: published 2026-10-08, adopted 2026-10-08: 0 days old/);
});

test("a missing notice and a weakened .npmrc are refused", () => {
  const input = real();
  input.notices = input.notices.replaceAll("tinykeys", "something-else");
  input.npmrc = input.npmrc.replace("ignore-scripts=true", "ignore-scripts=false");
  const text = names(failures(input));
  assert.match(text, /notices tinykeys/);
  assert.match(text, /\.npmrc: ignore-scripts=true/);
  assert.ok(REQUIRED_NPMRC.includes("save-exact=true"));
});

// A stand-in for the registry that tells the truth about EXPECTED; each test then lies about one thing.
const truthful = (name) => {
  const want = EXPECTED[name];
  return { versions: { [want.version]: { dist: { integrity: want.integrity }, license: want.license } }, time: { [want.version]: want.published + "T10:00:00.000Z" } };
};

test("the registry check passes when the registry agrees", async () => {
  const rows = await checkRegistry({ getDoc: async (name) => truthful(name) });
  assert.deepEqual(rows.filter((r) => !r.ok), []);
});

test("the registry check refuses a different hash, a deprecation and a missing version", async () => {
  const lie = (patch) => async (name) => {
    const doc = truthful(name);
    if (name === "tinykeys") patch(doc.versions["4.0.1"], doc);
    return doc;
  };
  const hash = await checkRegistry({ getDoc: lie((v) => { v.dist.integrity = "sha512-other"; }) });
  assert.match(names(hash.filter((r) => !r.ok)), /registry tinykeys: registry says sha512-other/);
  const old = await checkRegistry({ getDoc: lie((v) => { v.deprecated = "use something else"; }) });
  assert.match(names(old.filter((r) => !r.ok)), /deprecated: use something else/);
  const gone = await checkRegistry({ getDoc: lie((v, doc) => { delete doc.versions["4.0.1"]; }) });
  assert.match(names(gone.filter((r) => !r.ok)), /version 4\.0\.1 is not published/);
});

test("the registry check refuses a version published later than EXPECTED says", async () => {
  const rows = await checkRegistry({ getDoc: async (name) => {
    const doc = truthful(name);
    if (name === "lenis") doc.time["1.3.26"] = "2026-10-05T10:00:00.000Z"; // 3 days before it was adopted
    return doc;
  } });
  assert.match(names(rows.filter((r) => !r.ok)), /registry lenis: published 2026-10-05, EXPECTED says 2026-08-05/);
});

test("THIRD_PARTY_NOTICES.md is what tools/notices.mjs writes, and holds the text of every shipped licence", () => {
  const disk = read("THIRD_PARTY_NOTICES.md").replace(/\r\n/g, "\n");
  assert.equal(disk.trimEnd(), buildNotices().replace(/\r\n/g, "\n").trimEnd(), `${NOTICES} is stale: run node tools/notices.mjs`);
  for (const [name, want] of Object.entries(EXPECTED)) if (want.ships) assert.ok(disk.includes(`### ${name} ${want.version}`), name);
  for (const family of ["Barlow Condensed", "Instrument Serif", "Geist Mono", "Inter"]) assert.ok(disk.includes(`### ${family}`), family);
  assert.equal(disk.split("SIL OPEN FONT LICENSE Version 1.1").length - 1, 4);
  assert.ok(disk.includes("Permission to use, copy, modify, and/or distribute this software for any")); // Lucide's ISC
  assert.ok(disk.includes("Cole Bemis")); // the MIT part of Lucide's licence (icons derived from Feather)
});

test("VENDORED.md has a row for every approved package: name, version, licence and the same sha512", () => {
  const doc = read("VENDORED.md");
  for (const [name, want] of Object.entries(EXPECTED)) {
    const row = doc.split("\n").find((line) => line.startsWith(`| ${name} | ${want.version} |`));
    assert.ok(row, `no row for ${name} ${want.version}`);
    assert.ok(row.includes(` ${want.license} |`) && row.includes(want.integrity), `${name}: licence or hash differs from the table in check-lock.mjs`);
  }
});
