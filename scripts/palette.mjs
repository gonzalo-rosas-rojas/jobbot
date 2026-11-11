// Honest palette, built in OKLCH. Prints the tokens as sRGB hex (gamut-mapped by lowering chroma only,
// so lightness and hue stay as designed), checks WCAG contrast for every text/background pair the panel
// uses, and simulates the three kinds of colour blindness to check that states differ by lightness too.

const clamp01 = (x) => Math.min(1, Math.max(0, x));
const toLin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toGam = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

function oklchToLinear(L, C, Hdeg) {
  const h = (Hdeg * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = L - 0.0894841775 * a - 1.291485548 * b;
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}
const inGamut = (rgb) => rgb.every((v) => v >= -0.0005 && v <= 1.0005);

function toHex(L, C, H) {
  let c = C;
  let lin = oklchToLinear(L, c, H);
  while (!inGamut(lin) && c > 0) {
    c -= 0.002;
    lin = oklchToLinear(L, Math.max(0, c), H);
  }
  const rgb = lin.map((v) => clamp01(toGam(clamp01(v))));
  return { hex: "#" + rgb.map((v) => Math.round(v * 255).toString(16).padStart(2, "0")).join(""), lin: lin.map(clamp01), c };
}

const lum = (lin) => 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
const contrast = (a, b) => {
  const [hi, lo] = [lum(a.lin), lum(b.lin)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

// Machado et al. 2009, severity 1.0, in linear RGB
const CVD = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.011820, 0.042940, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.303900]],
};
const sim = (lin, m) => m.map((row) => clamp01(row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]));
function linToOklab(lin) {
  const [r, g, b] = lin;
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}
const dE = (a, b) => { const x = linToOklab(a), y = linToOklab(b); return Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]) * 100; };

// L, C, H
const MODES = {
  papel: {
    paper: [0.955, 0.008, 255], surface: [0.982, 0.005, 255], raise: [1, 0, 0], ink: [0.24, 0.045, 262], muted: [0.46, 0.035, 258],
    line: [0.86, 0.014, 255], "line-soft": [0.915, 0.01, 255],
    track: [0.5, 0.14, 255], "track-soft": [0.925, 0.04, 250], "track-ink": [0.36, 0.1, 258],
    gold: [0.84, 0.165, 85], "gold-ink": [0.24, 0.045, 262], "gold-soft": [0.955, 0.05, 92], "gold-text": [0.43, 0.095, 70],
    bad: [0.545, 0.19, 25], "bad-soft": [0.95, 0.03, 22], "on-bad": [1, 0, 0], ok: [0.4, 0.115, 163], "ok-soft": [0.95, 0.04, 165],
    board: [0.24, 0.045, 262], "board-hi": [0.3, 0.045, 262], "board-lo": [0.2, 0.04, 262], "board-edge": [0.34, 0.04, 262],
    chalk: [0.96, 0.01, 255], "chalk-dim": [0.72, 0.03, 258], gap: [0.12, 0.03, 262], urgent: [0.56, 0.2, 25],
  },
  tinta: {
    paper: [0.175, 0.025, 262], surface: [0.215, 0.028, 262], raise: [0.255, 0.03, 262], ink: [0.95, 0.012, 255], muted: [0.74, 0.03, 255],
    line: [0.33, 0.035, 260], "line-soft": [0.265, 0.03, 260],
    track: [0.73, 0.12, 245], "track-soft": [0.3, 0.05, 250], "track-ink": [0.86, 0.07, 245],
    gold: [0.84, 0.165, 85], "gold-ink": [0.22, 0.04, 262], "gold-soft": [0.3, 0.06, 88], "gold-text": [0.88, 0.14, 88],
    bad: [0.68, 0.17, 25], "bad-soft": [0.255, 0.055, 22], "on-bad": [0.16, 0.03, 262], ok: [0.86, 0.15, 165], "ok-soft": [0.29, 0.06, 165],
    board: [0.22, 0.04, 262], "board-hi": [0.275, 0.04, 262], "board-lo": [0.18, 0.035, 262], "board-edge": [0.35, 0.04, 262],
    chalk: [0.96, 0.01, 255], "chalk-dim": [0.72, 0.03, 258], gap: [0.1, 0.03, 262], urgent: [0.7, 0.18, 25],
  },
};

const PAIRS = [
  // [text, background, minimum, label]
  ["ink", "paper", 7, "body on page"], ["ink", "surface", 7, "body on card"], ["muted", "paper", 4.5, "secondary on page"],
  ["muted", "surface", 4.5, "secondary on card"], ["track", "paper", 4.5, "link on page"], ["track", "surface", 4.5, "link on card"],
  ["track-ink", "track-soft", 4.5, "blue ticket"], ["gold-text", "gold-soft", 4.5, "gold ticket"], ["gold-ink", "gold", 7, "text on gold"],
  ["bad", "paper", 4.5, "late text"], ["bad", "bad-soft", 4.5, "late ticket"], ["ok", "ok-soft", 4.5, "ok chip"], ["ok", "surface", 4.5, "ok text"],
  ["on-bad", "bad", 4.5, "text on red"], ["chalk", "board", 7, "board text"], ["chalk-dim", "board", 4.5, "board labels"],
  ["chalk", "board-hi", 7, "flap text"], ["line", "paper", 1.3, "hairline (decorative)"],
];
const STATES = ["track", "gold", "ok", "bad"]; // the four meanings: waits, your turn, confirmed, late

const out = {};
for (const [name, spec] of Object.entries(MODES)) {
  const made = {};
  for (const [k, [L, C, H]] of Object.entries(spec)) made[k] = toHex(L, C, H);
  out[name] = made;
  console.log(`\n== ${name} ==`);
  console.log(Object.entries(made).map(([k, v]) => `${k}: ${v.hex}`).join("   "));
  console.log("\ncontrast");
  for (const [t, b, min, label] of PAIRS) {
    const r = contrast(made[t], made[b]);
    console.log(`${r >= min ? "ok  " : "FAIL"} ${r.toFixed(1).padStart(5)}:1 (min ${min})  ${label}  ${made[t].hex} on ${made[b].hex}`);
  }
  console.log("\ncolour-blind separation of the four meanings on the page background (OKLab dE, >=12 is easy; lightness gap in brackets)");
  for (const kind of ["none", "protan", "deutan", "tritan"]) {
    const row = [];
    for (let i = 0; i < STATES.length; i++) for (let j = i + 1; j < STATES.length; j++) {
      const a = kind === "none" ? made[STATES[i]].lin : sim(made[STATES[i]].lin, CVD[kind]);
      const b = kind === "none" ? made[STATES[j]].lin : sim(made[STATES[j]].lin, CVD[kind]);
      row.push(`${STATES[i]}/${STATES[j]} ${dE(a, b).toFixed(0)}`);
    }
    console.log(kind.padEnd(7), row.join("  "));
  }
}
if (process.argv[2] === "css") {
  for (const [name, made] of Object.entries(out)) {
    console.log(`\n/* ${name} */`);
    console.log(Object.entries(made).map(([k, v]) => `--${k}: ${v.hex};`).join(" "));
  }
}
