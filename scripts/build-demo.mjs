// Builds docs/demo/index.html: the panel page with made-up data and no server, in English by default.
//   node scripts/build-demo.mjs
// The real work is panel-src/build.mjs (`demo --publish`): it builds the page from its sources, adds scripts/demo/demo.js
// in front of the page's own script, takes out the nonces that only the real server fills in and writes the file. Open the
// result straight from disk or serve docs/ as static files; `?lang=es` opens it in Spanish.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = join(dirname(fileURLToPath(import.meta.url)), "..", "panel-src");
execFileSync(process.execPath, ["build.mjs", "demo", "--publish"], { cwd: src, stdio: "inherit" });
