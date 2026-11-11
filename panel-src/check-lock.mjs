// The supply-chain gate for panel-src (docs/research/2026-10-07-revision/R8-reutilizar-con-seguridad.md section 2 rules
// 1-6 and section 5 steps 1-3 and 6; R8b section 7). It fails, with the reason, when:
//   - package.json has a range instead of an exact version, or a package that EXPECTED below does not list (or the reverse);
//   - package-lock.json disagrees with EXPECTED on a version or on the sha512 of a tarball, holds a package nobody
//     approved (every extra entry must be esbuild's own platform binary), resolves from anywhere but registry.npmjs.org,
//     or has an install script in any package but esbuild;
//   - a licence is outside the allow-list, read from the package.json that is really installed;
//   - a package was less than 7 days old when it entered the project (the quarantine; npm 11.1 has no min-release-age);
//   - THIRD_PARTY_NOTICES.md does not name a package that reaches the page, or .npmrc lost one of its settings.
// EXPECTED is a second source: its hashes and dates were taken from the registry with a separate query
// (`npm view <pkg>@<version> dist.integrity` and `time`), not from the lockfile.
//
//   node check-lock.mjs             offline checks, then asks the registry again for every hash, date, licence and deprecation
//   node check-lock.mjs --offline   offline checks only
//
// To add or raise a package: read `npm diff`, wait out the quarantine, `npm view`, put the new line here, extend
// THIRD_PARTY_NOTICES.md, and keep it out of any commit that changes behaviour (R8 §2 rule 9).

import { readFileSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

export const QUARANTINE_DAYS = 7;
export const ALLOWED_LICENSES = new Set(["MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause", "0BSD", "Unlicense", "CC0-1.0", "OFL-1.1", "Apache-2.0"]); // Apache-2.0 only with its NOTICE copied (none today)
export const REQUIRED_NPMRC = ["save-exact=true", "ignore-scripts=true", "audit=false", "fund=false", "update-notifier=false"];
const INSTALL_SCRIPTS_ALLOWED = ["node_modules/esbuild"];
const REGISTRY = "https://registry.npmjs.org/";

// `published` is the registry's date for that exact version, `since` the day the version entered the project, `ships`
// whether any of it reaches the page (esbuild only builds it). esbuild entered on 2026-10-07; the rest are the foundations
// of 2026-10-08. lucide-static 1.53.0 was published the same day, so it waited: 1.49.0 is the newest release older than 7
// days. three, animejs and simplex-noise were here until nothing imported them (the tower is archived, the foundations
// lab is gone); to bring one back is a new package, through the steps above.
export const EXPECTED = {
  esbuild: { version: "0.28.2", ships: false, license: "MIT", published: "2026-08-08", since: "2026-10-07", integrity: "sha512-HKVLS8dvII+xoKW9kmqxbRKrnWEXfJJr/FZhhJmiqIB0e053QNYFqOBouTMO/k5sID4MvCiUCvv8b9M4h32wIA==" },
  "@esbuild/win32-x64": { version: "0.28.2", ships: false, license: "MIT", published: "2026-08-08", since: "2026-10-07", integrity: "sha512-5ebpxr3nWMzrL/rnUI755Jkuee0bHL/Gq0WTF9lvcpv73wAp5eu8MfBUgWK9bhWvZjj7yX8etf/8tI8Ney695g==", transitive: true },
  lenis: { version: "1.3.26", ships: true, license: "MIT", published: "2026-08-05", since: "2026-10-08", integrity: "sha512-s/xTCZCxTFvHbAN1OzuhNaN5YPJH2ail0XAkctKW1b+RUAG4nUL5UHLXwNko1h8aEeT2jspBXegMgPJd8zcuag==" },
  tinykeys: { version: "4.0.1", ships: true, license: "MIT", published: "2026-09-25", since: "2026-10-08", integrity: "sha512-rClxhLQl+I5UMB5Ny7sfnZqDNa51u7h7h4gJV2UWslwADHEw75IuVPCnWrpO/otM7jHrHpwcZdRHd+//SiI55A==" },
  "@leeoniya/ufuzzy": { version: "1.0.19", ships: true, license: "MIT", published: "2025-08-22", since: "2026-10-08", integrity: "sha512-0pikDeYt0IHEUPza5RTCDXc/17S1pTrYnReEMp8Aa6k1ovzw5QdZLwicW8TjljwEZRb6oYag0xmALohrcq/yOQ==" },
  "lucide-static": { version: "1.49.0", ships: true, license: "ISC", published: "2026-09-29", since: "2026-10-08", integrity: "sha512-gSjtl7bR2v26oDInh2Wua1PrF2BRRXl9inhnLWcFfnRqRShmgZvlFRisPHqTfU7kxsuZU8ThNjAFotbInrzKow==" },
};

const EXACT = /^\d+\.\d+\.\d+$/;
const DAY = 86400000;
const days = (from, to) => Math.round((Date.parse(to) - Date.parse(from)) / DAY);
const today = () => new Date().toISOString().slice(0, 10);

/** Is every alternative of an SPDX expression on the allow-list? (Conservative: OR is treated like AND.) */
export function licenseAllowed(expression) {
  const tokens = String(expression || "").replace(/[()]/g, "").split(/\s+(?:OR|AND)\s+/).map((t) => t.trim()).filter(Boolean);
  return tokens.length > 0 && tokens.every((t) => ALLOWED_LICENSES.has(t));
}

/**
 * The offline checks, on plain data so a test can bend each input. `licenseOf(name)` returns the licence found in the
 * installed package.json (or undefined when it is not installed), `notices` and `npmrc` are file texts.
 * Returns rows { ok, id, note }; the gate passes when none has ok === false.
 */
export function checkOffline({ pkg, lock, notices, npmrc, licenseOf = () => undefined, expected = EXPECTED }) {
  const rows = [];
  const check = (ok, id, note) => rows.push({ ok: !!ok, id, note });

  // package.json: exact versions, and exactly the packages the table knows
  const declared = { ...pkg.dependencies, ...pkg.devDependencies };
  for (const [name, range] of Object.entries(declared)) check(EXACT.test(range), `package.json ${name}`, EXACT.test(range) ? range : `"${range}" is not an exact version`);
  const direct = Object.keys(expected).filter((n) => !expected[n].transitive);
  for (const name of direct) if (!(name in declared)) check(false, `package.json ${name}`, "listed in EXPECTED but not declared");
  for (const name of Object.keys(declared)) if (!direct.includes(name)) check(false, `package.json ${name}`, "declared but not approved in EXPECTED");
  const root = lock.packages[""] || {};
  check(JSON.stringify(root.dependencies || {}) === JSON.stringify(pkg.dependencies || {}) && JSON.stringify(root.devDependencies || {}) === JSON.stringify(pkg.devDependencies || {}), "lock in step", "package-lock.json and package.json declare the same dependencies");

  // the lock: version and sha512 of every approved package
  for (const [name, want] of Object.entries(expected)) {
    const got = lock.packages["node_modules/" + name];
    const ok = !!got && got.version === want.version && got.integrity === want.integrity;
    check(ok, `lock ${name}`, got ? `${got.version}${ok ? "" : " (version or sha512 differs from EXPECTED)"}` : "missing from the lock");
    if (declared[name] !== undefined && declared[name] !== want.version) check(false, `package.json ${name}`, `${declared[name]} but EXPECTED says ${want.version}`);
  }
  // everything else in the lock must be a platform binary of the pinned esbuild, with a hash
  const esbuildVersion = expected.esbuild && expected.esbuild.version;
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key) continue;
    const name = key.replace(/^node_modules\//, "");
    if (expected[name]) continue;
    const platform = /^@esbuild\/[a-z0-9-]+$/.test(name) && entry.version === esbuildVersion && !!entry.integrity;
    check(platform, `lock ${name}`, platform ? "esbuild platform binary" : "a package nobody approved");
  }
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (key && entry.resolved && !entry.resolved.startsWith(REGISTRY)) check(false, `lock ${key.replace(/^node_modules\//, "")}`, `resolves from ${entry.resolved}`);
  }
  const withScripts = Object.entries(lock.packages).filter(([, e]) => e.hasInstallScript).map(([k]) => k).sort();
  check(JSON.stringify(withScripts) === JSON.stringify(INSTALL_SCRIPTS_ALLOWED), "install scripts", withScripts.length ? withScripts.join(", ") : "none");

  // licences: the installed package.json when there is one, the lock otherwise
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key) continue;
    const name = key.replace(/^node_modules\//, "");
    const found = licenseOf(name) ?? entry.license;
    check(licenseAllowed(found), `licence ${name}`, String(found));
    if (expected[name] && found !== expected[name].license) check(false, `licence ${name}`, `${found} but EXPECTED says ${expected[name].license}`);
  }

  // the quarantine, as recorded at the moment each version entered the project
  for (const [name, want] of Object.entries(expected)) {
    const age = days(want.published, want.since);
    check(age >= QUARANTINE_DAYS, `quarantine ${name}`, `published ${want.published}, adopted ${want.since}: ${age} days old`);
    check(want.since <= today(), `quarantine ${name}`, `adopted on ${want.since}, which is not in the future`);
  }

  // notices and .npmrc
  for (const [name, want] of Object.entries(expected)) {
    if (want.ships) check(notices.includes(name) && notices.includes(want.version), `notices ${name}`, `${name} ${want.version} in THIRD_PARTY_NOTICES.md`);
  }
  const lines = npmrc.split(/\r?\n/).map((l) => l.trim());
  for (const setting of REQUIRED_NPMRC) check(lines.includes(setting), `.npmrc`, setting);
  return rows;
}

/** The registry's own answer for every approved version: hash, date, licence, deprecation. `getDoc(name)` returns the packument. */
export async function checkRegistry({ getDoc, expected = EXPECTED }) {
  const rows = [];
  const check = (ok, id, note) => rows.push({ ok: !!ok, id, note });
  for (const [name, want] of Object.entries(expected)) {
    let doc;
    try {
      doc = await getDoc(name);
    } catch (e) {
      check(false, `registry ${name}`, `could not be read: ${e.message}`);
      continue;
    }
    const v = doc.versions && doc.versions[want.version];
    if (!v) { check(false, `registry ${name}`, `version ${want.version} is not published`); continue; }
    const published = String(doc.time && doc.time[want.version]).slice(0, 10);
    check(v.dist.integrity === want.integrity, `registry ${name}`, v.dist.integrity === want.integrity ? "sha512 matches" : `registry says ${v.dist.integrity}`);
    check(published === want.published, `registry ${name}`, `published ${published}` + (published === want.published ? "" : `, EXPECTED says ${want.published}`));
    check(days(published, want.since) >= QUARANTINE_DAYS, `registry ${name}`, `${days(published, want.since)} days old when adopted`);
    check(v.license === want.license, `registry ${name}`, `licence ${v.license}`);
    check(!v.deprecated, `registry ${name}`, v.deprecated ? `deprecated: ${v.deprecated}` : "not deprecated");
  }
  return rows;
}

function readLicense(name) {
  const file = path.join(here, "node_modules", name, "package.json");
  if (!existsSync(file)) return undefined;
  const l = JSON.parse(readFileSync(file, "utf8")).license;
  return typeof l === "object" && l ? l.type : l;
}

async function getDoc(name) {
  const res = await fetch(REGISTRY + name.replace("/", "%2f"), { headers: { "user-agent": "jobbot-panel check-lock.mjs" } });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json();
}

async function main() {
  const offlineOnly = process.argv.includes("--offline");
  const read = (f) => readFileSync(path.join(here, f), "utf8");
  const rows = checkOffline({
    pkg: JSON.parse(read("package.json")),
    lock: JSON.parse(read("package-lock.json")),
    notices: existsSync(path.join(here, "THIRD_PARTY_NOTICES.md")) ? read("THIRD_PARTY_NOTICES.md") : "",
    npmrc: existsSync(path.join(here, ".npmrc")) ? read(".npmrc") : "",
    licenseOf: readLicense,
  });
  if (offlineOnly) console.log("registry: skipped (--offline)");
  else rows.push(...(await checkRegistry({ getDoc })));
  const bad = rows.filter((r) => !r.ok);
  for (const r of rows) if (!r.ok || process.argv.includes("--verbose")) console.log(`${r.ok ? "ok " : "BAD"} ${r.id}: ${r.note}`);
  const names = Object.entries(EXPECTED).map(([n, w]) => `${n}@${w.version}`);
  console.log(`${bad.length ? "FAILED" : "ok"}: ${rows.length - bad.length}/${rows.length} checks over ${names.length} approved packages${offlineOnly ? " (offline)" : ""}`);
  process.exit(bad.length ? 1 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
