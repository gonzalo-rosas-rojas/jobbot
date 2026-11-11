// Obviously fictional data for the fixtures (AGENT-RULES: fictional examples only in labs and the demo): invented company names built from
// syllables, generic job titles, and a seeded random so every run and every test sees the same lines.

/** A small seeded generator (mulberry32): same seed, same sequence, values in [0, 1). */
export function seeded(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Syllables chosen to be unlikely as real names, so no line can be mistaken for an actual company.
const HEAD = ["Zy", "Qua", "Vri", "Xe", "Kyo", "Oru", "Pho", "Blu", "Yav", "Thu", "Jex", "Wyn", "Ixa", "Nuo", "Fry"];
const TAIL = ["vex", "tron", "nik", "lume", "grix", "forge", "quill", "wix", "loop", "bay", "rook", "spar"];
const KIND = ["Labs", "Systems", "Works", "Studio", "Cloud", "Dynamics", "Freight", "Health"];
const ROLE = ["Backend Engineer", "Platform Engineer", "Data Engineer", "Rust Developer", "Frontend Engineer", "Site Reliability Engineer", "QA Automation Engineer", "Solutions Engineer", "Security Engineer", "Mobile Developer"];
const WHERE = ["Remote", "Remote (Americas)", "Remote (EU time zones)", "Hybrid", "Remote (anywhere)"];

/** `count` lines like "Rust Developer - Oruwix Cloud - Remote (Americas)", deterministic for a seed. */
export function fakePostings(count = 300, seed = 20261008) {
  const rand = seeded(seed);
  const pick = (list) => list[Math.floor(rand() * list.length)];
  return Array.from({ length: count }, () => `${pick(ROLE)} - ${pick(HEAD)}${pick(TAIL)} ${pick(KIND)} - ${pick(WHERE)}`);
}
