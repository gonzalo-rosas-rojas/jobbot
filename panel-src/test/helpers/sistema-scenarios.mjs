// test/helpers/sistema-scenarios.mjs: the scenarios of ./sistema-core.mjs on the three fixtures of test/fixtures, read from disk. For the Node tests;
// the lab (lab/f2-sistema) builds the same ones from the same files through the bundler.
//
//   scenario("night") | "stale" | "never" | "stop" | "heavy" | "day"
import { readFileSync } from "node:fs";
import { scenariosOf } from "./sistema-core.mjs";

export * from "./sistema-core.mjs";
const fixture = (name) => JSON.parse(readFileSync(new URL(`../fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));
export const SCENARIOS = scenariosOf({ rich: fixture("rich"), empty: fixture("empty"), heavy: fixture("heavy") });
export const scenario = (name) => SCENARIOS[name]();
