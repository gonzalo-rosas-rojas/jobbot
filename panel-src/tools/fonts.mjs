// Downloads, verifies, subsets, sanitises and records the panel's fonts. Node only, no dependencies; the font tools
// run through uvx (fonttools + brotli for the subset and the Inter instances, opentype-sanitizer for OTS), so nothing
// is installed globally.
//
//   node tools/fonts.mjs            -> assets/fonts/<face>.woff2, assets/fonts/licenses/*.txt, assets/fonts/VENDORED.md
//   node tools/fonts.mjs hashes     -> downloads only and prints the manifest lines (sha256, git blob, size) to pin
//   node tools/fonts.mjs --offline  -> skips the second-origin query to the GitHub API (the pinned blob sha still applies)
//   node tools/fonts.mjs check      -> writes nothing: the files on disk against VENDORED.md and the manifest
//   node tools/fonts.mjs check --online -> also downloads every original again (fresh folder) and checks both fingerprints
//
// Why this exists (docs/research/2026-10-07-revision/R8-reutilizar-con-seguridad.md, §2 rule 10 and §5 step 7; R8b
// §2.2 and §7 step 7): fonts come from the official repository at a fixed commit, are downloaded once into an empty
// folder that git ignores, are checked against two independent sources before use (the sha256 pinned here after the
// first download, and the git blob sha the GitHub API publishes for that commit), are reduced with pyftsubset (which
// drops hinting and variation tables, where the recent font CVEs live) and are accepted only if OTS, the same sanitiser
// the browsers run, says yes. Every byte that enters assets/ gets a row in VENDORED.md.
//
// The manifest below is the only thing to edit when a font changes: pin a new ref, run `hashes`, paste the lines.

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.join(here, "..");
export const FONTS_DIR = path.join(ROOT, "assets", "fonts");
export const LICENSES_DIR = path.join(FONTS_DIR, "licenses");
export const VENDORED = path.join(FONTS_DIR, "VENDORED.md");
const VENDOR_SRC = path.join(ROOT, ".vendor-src");

// Official origins, pinned. google/fonts has no tags, so the ref is the head commit of `main` on the day of pinning;
// vercel/geist-font is pinned by the commit its release tag pointed at (a tag can be moved, a commit cannot).
export const SOURCES = {
  googleFonts: {
    repo: "google/fonts",
    ref: "2eb0b48d5f760f62e286216f0859a8c540dbc1bd",
    label: "google/fonts, commit 2eb0b48d (main on 2026-10-08)",
  },
  geist: {
    repo: "vercel/geist-font",
    ref: "a73329da8fc62afc917f796555202e4997f79b7c",
    tag: "v1.7.2",
    label: "vercel/geist-font, tag v1.7.2 = commit a73329da (2026-06-01)",
  },
};

// Every file that crosses the network, with the two fingerprints each download must match: `blob` is the git blob
// sha1 the GitHub contents API reported for that path at that commit (second origin, fetched 2026-10-08), `sha256`
// is the hash of the bytes as first downloaded. `size` is in bytes.
export const FILES = {
  "ofl/barlowcondensed/BarlowCondensed-Bold.ttf": {
    source: "googleFonts", size: 109912,
    blob: "9e480f467d703009e3abfce290b2ec2b7d24e188",
    sha256: "e476562ec9c1e16cf16475895b511f08c804f438cc9a9f80a44ea50a0eeb5b65",
  },
  "ofl/barlowcondensed/OFL.txt": {
    source: "googleFonts", size: 4377,
    blob: "2f22ba6ca4dbc12145ae40b14c3368f387cba0e2",
    sha256: "186d750eb496a4c17a76385f82be6aea2ac1cf2de074a811d63786cf374ea73f",
  },
  "ofl/instrumentserif/InstrumentSerif-Italic.ttf": {
    source: "googleFonts", size: 71592,
    blob: "192c3eda9864241ad280e9bcc84e258c80a4a588",
    sha256: "08939b8bdf534afec24ae0ef5e03f948940cd9a8fe08e7fecbad040e62327385",
  },
  "ofl/instrumentserif/OFL.txt": {
    source: "googleFonts", size: 4405,
    blob: "669ce7917bec662fcf7244d37f49c4277f6fbd73",
    sha256: "129ed7618959716959f2941fdd5b49e0ad6e6c1d78726761786a00253d865521",
  },
  "fonts/GeistMono/ttf/GeistMono-Regular.ttf": {
    source: "geist", size: 149284,
    blob: "643e6ba0731882a61296da368cb0d6362290b882",
    sha256: "5a0de4b3d54ab272f76a1d8c84b7fb24c67bbec6591d5300e61c7bc10094b6c8",
  },
  "LICENSE.txt": {
    source: "geist", size: 4368,
    blob: "8d003fec6b8a0f9f866ab1038579b6ea20926ac4",
    sha256: "930853ee1daa68554d9e35c8a9175affb74f699fad9a5da6ee5ebe76379d9137",
  },
  // Inter comes as the variable font and is instanced here. The rsms/inter release (Inter-4.1.zip) was the other
  // option: 33.7 MB for two static faces, no digest published by the release API, and its statics are built from the
  // same masters, so after `--no-hinting` the subsets would weigh the same. 877 KB with a blob sha wins.
  "ofl/inter/Inter[opsz,wght].ttf": {
    source: "googleFonts", size: 876576,
    blob: "047c92f6e2212473dc436020afed689527076d44",
    sha256: "29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031",
  },
  "ofl/inter/OFL.txt": {
    source: "googleFonts", size: 4377,
    blob: "21f6aff961064c2e429f570995e446bcdd555422",
    sha256: "5b9321a4298cfeb6b34354164a1c3afc3db114569984c502b9b35d988fd58c57",
  },
};

// Basic Latin, Latin-1, Latin Extended-A, general punctuation (U+2000-206F holds the bullet U+2022 and the ellipsis
// U+2026), currency, arrows. Enough for Spanish and English UI text, numbers, money and the arrows the panel draws.
export const UNICODES = "U+0020-007E,U+00A0-00FF,U+0100-017F,U+2000-206F,U+20A0-20CF,U+2190-21FF";

const INTER_FEATURES = "kern,mark,mkmk,cpsp,calt,ccmp,locl,liga,case,tnum,pnum,zero";

// The faces the page will declare. `max` is the byte ceiling test/fonts.test.mjs enforces per face (measured subset
// plus a little slack); `features` overrides the default `*` of pyftsubset's --layout-features; TOTAL_BUDGET below is
// the budget R8 §1 set for all five together.
export const FACES = [
  { file: "barlow-condensed-700.woff2", family: "Barlow Condensed", weight: 700, style: "normal",
    from: "ofl/barlowcondensed/BarlowCondensed-Bold.ttf", license: "ofl/barlowcondensed/OFL.txt", licenseFile: "OFL-barlow-condensed.txt", max: 24 * 1024 },
  { file: "instrument-serif-400italic.woff2", family: "Instrument Serif", weight: 400, style: "italic",
    from: "ofl/instrumentserif/InstrumentSerif-Italic.ttf", license: "ofl/instrumentserif/OFL.txt", licenseFile: "OFL-instrument-serif.txt", max: 24 * 1024 },
  { file: "geist-mono-400.woff2", family: "Geist Mono", weight: 400, style: "normal",
    from: "fonts/GeistMono/ttf/GeistMono-Regular.ttf", license: "LICENSE.txt", licenseFile: "OFL-geist-mono.txt", max: 24 * 1024 },
  // opsz 14 is Inter's text instance (the axis runs 14-32); the panel sets type between 11 and 18 px.
  // Inter is the one face that cannot keep every layout feature: `*` closes over its eight stylistic sets, fourteen
  // character variants and the numeric alternates, 1,014 glyphs and 35.6 KB per weight (measured 2026-10-08, see
  // VENDORED.md), which puts the five faces at ~125 KB, over this budget and over R8 §2 rule 10's 120 KB cap. The list
  // below keeps what a browser applies on its own (kern, mark, mkmk, cpsp, calt, ccmp, locl, liga) plus the four
  // features the panel's CSS may ask for: tabular and proportional figures, the slashed zero and case-sensitive forms.
  { file: "inter-400.woff2", family: "Inter", weight: 400, style: "normal",
    from: "ofl/inter/Inter[opsz,wght].ttf", instance: { wght: 400, opsz: 14 }, features: INTER_FEATURES,
    license: "ofl/inter/OFL.txt", licenseFile: "OFL-inter.txt", max: 28 * 1024 },
  { file: "inter-600.woff2", family: "Inter", weight: 600, style: "normal",
    from: "ofl/inter/Inter[opsz,wght].ttf", instance: { wght: 600, opsz: 14 }, features: INTER_FEATURES,
    license: "ofl/inter/OFL.txt", licenseFile: "OFL-inter.txt", max: 28 * 1024 },
];
export const TOTAL_BUDGET = 110 * 1024;

// What the measurements said, kept for the day a room needs fractions or superscripts (then add
// ordn,sups,subs,frac,numr,dnom: +4.6 KB per weight) or everything (`*`: +12 KB per weight).
const INTER_NOTE = "layout features limited on purpose: with `*` this weight measured 35.6 KB (400) and 36.7 KB (600); " +
  "with the list above plus ordn,sups,subs,frac,numr,dnom it measured 28.3 KB and 29.1 KB; the three other faces keep `*` because they fit.";

const OFL_MARK = "SIL OPEN FONT LICENSE Version 1.1";
// The font tools are build-time only and pinned like everything else (R8 §2 rule 2). All three are older than the 7 day
// quarantine on 2026-10-08 (fonttools 4.66.1 was published 2026-09-29, brotli 1.2.0 on 2025-11-05, opentype-sanitizer 9.2.0
// on 2024-11-06; MIT, MIT and BSD-style).
export const TOOLS = { fonttools: "4.66.1", brotli: "1.2.0", ots: "9.2.0" };
// --exclude-newer freezes what uv may resolve (including anything these packages pull in) to the day the tools were chosen.
const EXCLUDE_NEWER = "2026-10-08T00:00:00Z";
const UVX_FONTTOOLS = ["--exclude-newer", EXCLUDE_NEWER, "--from", `fonttools==${TOOLS.fonttools}`, "--with", `brotli==${TOOLS.brotli}`];
const UVX_OTS = ["--exclude-newer", EXCLUDE_NEWER, "--from", `opentype-sanitizer==${TOOLS.ots}`];
// fonttools stamps head.modified with the clock and orders some GPOS lookups by set iteration, so the Inter instances
// differed from run to run. With both fixed, the same inputs give the same bytes on any machine.
const SOURCE_DATE_EPOCH = String(Date.UTC(2026, 9, 8) / 1000);
const REPRODUCIBLE_ENV = { PYTHONHASHSEED: "0", SOURCE_DATE_EPOCH };
// The sanitiser's wheel ships the ots-sanitize binary but no console script, so it is reached through its module.
const OTS_SHIM = "import ots, sys; r = ots.sanitize(*sys.argv[1:]); sys.exit(r.returncode)";

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");
// What git stores for a file: sha1 over "blob <size>\0<bytes>". It lets a download be checked against the API's tree.
const gitBlob = (buf) => createHash("sha1").update(`blob ${buf.length}\0`).update(buf).digest("hex");
const kb = (n) => (n / 1024).toFixed(1) + " KB";
const rawUrl = (file) => `https://raw.githubusercontent.com/${SOURCES[FILES[file].source].repo}/${SOURCES[FILES[file].source].ref}/${file}`;
const apiUrl = (file) => `https://api.github.com/repos/${SOURCES[FILES[file].source].repo}/contents/${file}?ref=${SOURCES[FILES[file].source].ref}`;

function fail(msg) {
  console.error("fonts: " + msg);
  process.exit(1);
}

async function download(url) {
  const res = await fetch(url, { headers: { "user-agent": "jobbot-panel fonts.mjs" } });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText} for ${url}`);
  return Buffer.from(await res.arrayBuffer());
}

async function apiBlob(file) {
  const res = await fetch(apiUrl(file), { headers: { "user-agent": "jobbot-panel fonts.mjs", accept: "application/vnd.github+json" } });
  if (!res.ok) return { error: `${res.status} ${res.statusText}` };
  const json = await res.json();
  return { sha: json.sha, size: json.size };
}

// A fresh, empty folder per run: nothing is ever executed from it and nothing old can be mistaken for new.
function runFolder() {
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\.\d+Z$/, "Z");
  const dir = path.join(VENDOR_SRC, stamp);
  if (existsSync(dir)) fail(`${dir} already exists`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

async function fetchAll(dir, { offline, pinning }) {
  const got = {};
  for (const [file, want] of Object.entries(FILES)) {
    const url = rawUrl(file);
    const buf = await download(url);
    const hash = sha256(buf), blob = gitBlob(buf);
    const problems = [];
    if (buf.length !== want.size) problems.push(`size ${buf.length}, manifest says ${want.size}`);
    if (blob !== want.blob) problems.push(`git blob ${blob}, manifest (GitHub API) says ${want.blob}`);
    if (!pinning && hash !== want.sha256) problems.push(`sha256 ${hash}, manifest says ${want.sha256}`);
    let apiChecked = false;
    if (!offline) {
      const api = await apiBlob(file);
      if (api.error) console.warn(`  (GitHub API not consulted for ${file}: ${api.error}; the pinned blob sha still applies)`);
      else if (api.sha !== blob || api.size !== buf.length) problems.push(`GitHub API now reports blob ${api.sha} (${api.size} bytes)`);
      else apiChecked = true;
    }
    if (problems.length) fail(`${file} refused:\n - ${problems.join("\n - ")}`);
    // The whole path, flattened: three of the files are called OFL.txt and must not overwrite each other.
    const local = path.join(dir, want.source, file.replace(/\//g, "__"));
    mkdirSync(path.dirname(local), { recursive: true });
    writeFileSync(local, buf);
    got[file] = { url, local, size: buf.length, sha256: hash, blob, apiChecked };
    console.log(`${pinning ? "fetched" : "verified"} ${file} ${kb(buf.length)}`);
  }
  return got;
}

/** Problems found comparing what is on disk with VENDORED.md and the manifest (empty = consistent). No network. */
export function verifyLocal() {
  const problems = [];
  const text = readFileSync(VENDORED, "utf8");
  for (const face of FACES) {
    const file = path.join(FONTS_DIR, face.file);
    if (!existsSync(file)) { problems.push(`${face.file} is missing`); continue; }
    const buf = readFileSync(file);
    if (buf.subarray(0, 4).toString("latin1") !== "wOF2") problems.push(`${face.file} has no wOF2 magic`);
    const section = (text.split(`### \`${face.file}\``)[1] || "").split("\n### ")[0];
    const recorded = /- subset: (\d+) bytes .*sha256 `([0-9a-f]{64})`/.exec(section);
    if (!recorded) problems.push(`VENDORED.md has no subset line for ${face.file}`);
    else if (Number(recorded[1]) !== buf.length || recorded[2] !== sha256(buf)) problems.push(`${face.file} differs from what VENDORED.md records`);
    // Licences are stored with LF line endings (git on Windows may check them out with CRLF); the manifest pins the original bytes.
    const licence = path.join(LICENSES_DIR, face.licenseFile);
    if (!existsSync(licence)) problems.push(`licenses/${face.licenseFile} is missing`);
    else if (sha256(Buffer.from(readFileSync(licence, "utf8").replace(/\r\n/g, "\n"))) !== FILES[face.license].sha256) problems.push(`licenses/${face.licenseFile} is not the text of ${face.license}`);
  }
  return problems;
}

function licenseText(local) {
  const text = readFileSync(local, "utf8");
  if (!text.includes(OFL_MARK)) fail(`${local} is not the SIL Open Font License 1.1; refusing (R8 §2 rule 6)`);
  return text;
}

function run(cmd, args, what) {
  // cwd is panel-src, never the download folder: the tools only ever receive absolute paths as arguments.
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", windowsHide: true, env: { ...process.env, ...REPRODUCIBLE_ENV } });
  if (r.error) fail(`${what}: ${r.error.message}`);
  if (r.status !== 0) fail(`${what} exited ${r.status}\n${r.stdout}\n${r.stderr}`);
  return r;
}

// The commands go into VENDORED.md, which is committed: paths are shown relative to panel-src and the download folder by its
// role, so the file says what was run without saying whose machine ran it.
let runDir = null;
const slashes = (text) => text.replace(/\\/g, "/");
const portable = (text) => {
  let out = slashes(text);
  if (runDir) out = out.split(slashes(runDir)).join(".vendor-src/<download>");
  return out.split(slashes(ROOT) + "/").join("");
};
const show = (cmd, args) => [cmd, ...args.map((a) => (/[\s"\[\]]/.test(a) ? JSON.stringify(portable(a)) : portable(a)))].join(" ");

function instance(face, src, workDir) {
  const out = path.join(workDir, face.file.replace(".woff2", ".ttf"));
  const axes = Object.entries(face.instance).map(([k, v]) => `${k}=${v}`);
  const args = [...UVX_FONTTOOLS, "fonttools", "varLib.instancer", "-o", out, "--update-name-table", src, ...axes];
  run("uvx", args, `instancer ${face.file}`);
  return { out, command: show("uvx", args) };
}

function subset(face, src) {
  const out = path.join(FONTS_DIR, face.file);
  const args = [...UVX_FONTTOOLS, "pyftsubset", src,
    `--unicodes=${UNICODES}`, "--flavor=woff2", `--layout-features=${face.features || "*"}`, "--no-hinting", "--desubroutinize", `--output-file=${out}`];
  run("uvx", args, `pyftsubset ${face.file}`);
  const buf = readFileSync(out);
  if (buf.subarray(0, 4).toString("latin1") !== "wOF2") fail(`${face.file} is not a WOFF2 file`);
  return { out, buf, command: show("uvx", args) };
}

function sanitize(file) {
  const args = [...UVX_OTS, "python", "-I", "-c", OTS_SHIM, file];
  const r = run("uvx", args, `ots-sanitize ${path.basename(file)}`);
  const version = run("uvx", [...UVX_OTS, "python", "-I", "-c", "import ots; r = ots.sanitize('--version', capture_output=True); print(r.stdout.decode().strip())"], "ots version").stdout.trim();
  return { ok: r.status === 0, version, command: `uvx --exclude-newer ${EXCLUDE_NEWER} --from opentype-sanitizer==${TOOLS.ots} python -I -c "<ots.sanitize(sys.argv[1:])>" ${portable(file)}` };
}

function toolVersions() {
  const ft = run("uvx", [...UVX_FONTTOOLS, "python", "-I", "-c", "import fontTools, brotli; print(fontTools.version, brotli.__version__)"], "fonttools version").stdout.trim().split(" ");
  return { fonttools: ft[0], brotli: ft[1] };
}

function writeVendored(rows, licenses, tools, date) {
  const total = rows.reduce((n, r) => n + r.after, 0);
  const lines = [];
  lines.push("# Vendored fonts");
  lines.push("");
  lines.push(`Generated by \`node tools/fonts.mjs\` on ${date}. Do not edit by hand: change the manifest in \`tools/fonts.mjs\` and run it again.`);
  lines.push("Every file here came from the official repository at the pinned commit, was checked against two sources (the sha256 pinned");
  lines.push("in the manifest after the first download and the git blob sha the GitHub API publishes for that commit), was subset with");
  lines.push(`pyftsubset (fonttools ${tools.fonttools}, brotli ${tools.brotli}; hinting and variation tables dropped) and was accepted by OTS ${tools.ots}.`);
  lines.push("Rules followed: R8 §2 rule 6 (licences: OFL-1.1 only, text copied next to the font) and rule 10 (official origin, one download at");
  lines.push("build, sha256 before use, ots-sanitize and pyftsubset, 120 KB ceiling for all fonts); R8b §2.2 and §7 step 7.");
  lines.push("");
  lines.push(`Reproducible: the tools are pinned (fonttools ${TOOLS.fonttools}, brotli ${TOOLS.brotli}, opentype-sanitizer ${TOOLS.ots}) and run with`);
  lines.push(`PYTHONHASHSEED=0 and SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH}, so the same inputs give the same woff2 bytes on any machine.`);
  lines.push("");
  lines.push("## Sources");
  lines.push("");
  lines.push("| id | repository | pinned ref |");
  lines.push("|---|---|---|");
  for (const [id, s] of Object.entries(SOURCES)) lines.push(`| ${id} | https://github.com/${s.repo} | ${s.tag ? `tag ${s.tag} = ` : ""}commit ${s.ref} |`);
  lines.push("");
  lines.push(`Unicode ranges kept in every face: \`${UNICODES}\`.`);
  lines.push("");
  lines.push("## Faces");
  lines.push("");
  for (const r of rows) {
    lines.push(`### \`${r.face.file}\``);
    lines.push("");
    lines.push(`- family: ${r.face.family}, weight ${r.face.weight}, style ${r.face.style}`);
    lines.push(`- origin: ${SOURCES[FILES[r.face.from].source].label}, path \`${r.face.from}\``);
    lines.push(`- url: ${r.src.url}`);
    lines.push(`- licence: OFL-1.1, text in \`licenses/${r.face.licenseFile}\``);
    lines.push(`- original: ${r.src.size} bytes (${kb(r.src.size)}), sha256 \`${r.src.sha256}\`, git blob \`${r.src.blob}\`${r.src.apiChecked ? " (matched the GitHub API at run time)" : " (pinned from the GitHub API)"}`);
    lines.push(`- subset: ${r.after} bytes (${kb(r.after)}), sha256 \`${r.sha256}\`, layout features \`${r.face.features || "*"}\``);
    if (r.face.features) lines.push(`- note: ${INTER_NOTE}`);
    lines.push(`- ots-sanitize: ${r.ots.ok ? "ok" : "FAILED"} (${tools.ots})`);
    lines.push(`- date: ${date}`);
    lines.push("- commands:");
    lines.push("");
    lines.push("```");
    for (const c of r.commands) lines.push(c);
    lines.push("```");
    lines.push("");
  }
  lines.push("## Licences");
  lines.push("");
  lines.push("| file | origin | sha256 |");
  lines.push("|---|---|---|");
  for (const l of licenses) lines.push(`| \`licenses/${l.file}\` | ${l.url} | \`${l.sha256}\` |`);
  lines.push("");
  lines.push("## Totals");
  lines.push("");
  lines.push(`${rows.length} faces, ${total} bytes (${kb(total)}) of ${kb(TOTAL_BUDGET)} budgeted.`);
  lines.push("");
  writeFileSync(VENDORED, lines.join("\n"));
  return total;
}

async function main() {
  const args = process.argv.slice(2);
  const pinning = args.includes("hashes");
  const offline = args.includes("--offline");
  if (!pinning && Object.values(FILES).some((f) => f.sha256 === "__PIN__")) fail("the manifest has unpinned sha256 values: run `node tools/fonts.mjs hashes` and paste them");

  if (args[0] === "check") {
    const problems = verifyLocal();
    if (problems.length) fail(`check refused:\n - ${problems.join("\n - ")}`);
    console.log(`ok: ${FACES.length} faces and ${new Set(FACES.map((f) => f.licenseFile)).size} licences match VENDORED.md and the manifest`);
    if (args.includes("--online")) await fetchAll(runFolder(), { offline: false, pinning: false });
    return;
  }

  const dir = runFolder();
  runDir = dir;
  console.log(`downloading into ${path.relative(ROOT, dir)}`);
  const got = await fetchAll(dir, { offline, pinning });
  if (pinning) {
    console.log("\nmanifest lines:");
    for (const [file, g] of Object.entries(got)) console.log(`  ${JSON.stringify(file)}: size ${g.size}, blob "${g.blob}", sha256 "${g.sha256}"`);
    return;
  }

  mkdirSync(LICENSES_DIR, { recursive: true });
  const licenses = [];
  for (const face of FACES) {
    if (licenses.some((l) => l.file === face.licenseFile)) continue;
    const text = licenseText(got[face.license].local);
    writeFileSync(path.join(LICENSES_DIR, face.licenseFile), text.replace(/\r\n/g, "\n"));
    licenses.push({ file: face.licenseFile, url: got[face.license].url, sha256: got[face.license].sha256 });
  }

  const date = new Date().toISOString().slice(0, 10);
  const tools = toolVersions();
  const work = path.join(dir, "work");
  mkdirSync(work, { recursive: true });
  const rows = [];
  for (const face of FACES) {
    const commands = [];
    let src = got[face.from].local;
    if (face.instance) {
      const inst = instance(face, src, work);
      commands.push(inst.command);
      src = inst.out;
    }
    const sub = subset(face, src);
    commands.push(sub.command);
    const ots = sanitize(sub.out);
    commands.push(ots.command);
    tools.ots = ots.version;
    if (sub.buf.length > face.max) fail(`${face.file} is ${sub.buf.length} bytes, over its ${face.max} byte ceiling`);
    rows.push({ face, src: got[face.from], after: sub.buf.length, sha256: sha256(sub.buf), ots, commands });
    console.log(`${face.file} ${kb(got[face.from].size)} -> ${kb(sub.buf.length)} ots ${ots.ok ? "ok" : "FAILED"}`);
  }
  const total = writeVendored(rows, licenses, tools, date);
  if (total > TOTAL_BUDGET) fail(`all faces weigh ${kb(total)}, over the ${kb(TOTAL_BUDGET)} budget`);
  console.log(`total ${kb(total)} of ${kb(TOTAL_BUDGET)} -> ${path.relative(ROOT, VENDORED)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
