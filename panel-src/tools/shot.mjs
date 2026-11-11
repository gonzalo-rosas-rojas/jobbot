// One-shot screenshot of a URL from the command line, in its own headless Chrome.
//   node tools/shot.mjs <url> <out.png> [--w 1440] [--h 900] [--wait 1200] [--dark] [--light] [--reduced] [--eval "<js>"]
// Prints the page's console errors / CSP violations (if any) and the value of --eval (if given).
import { launch } from "./cdp.mjs";

const argv = process.argv.slice(2);
const [url, out] = argv;
const flag = (name) => argv.includes(name);
const val = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
if (!url || !out) {
  console.error("usage: node tools/shot.mjs <url> <out.png> [--w N] [--h N] [--wait ms] [--dark|--light] [--reduced] [--eval js]");
  process.exit(2);
}
const w = Number(val("--w", 1440));
const h = Number(val("--h", 900));
const b = await launch({ width: w, height: h });
try {
  const page = await b.newPage();
  if (flag("--dark") || flag("--light") || flag("--reduced")) {
    await page.media({ dark: flag("--dark") ? true : flag("--light") ? false : undefined, reducedMotion: flag("--reduced") ? true : undefined });
  }
  await page.goto(url);
  await page.wait(Number(val("--wait", 1200)));
  if (argv.includes("--eval")) console.log("eval:", JSON.stringify(await page.eval(val("--eval", "null"))));
  await page.shot(out);
  const problems = page.problems();
  console.log(problems.length ? "problems:\n" + problems.map((p) => ` - [${p.kind}] ${p.text}`).join("\n") : "no console errors");
  console.log("saved", out);
} finally {
  await b.close();
}
