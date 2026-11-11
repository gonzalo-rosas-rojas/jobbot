// Snapshots for APRENDER from the fixtures on disk (Node tests). The builders themselves are in aprender-scenarios.mjs, which has no
// imports so that the lab can bundle it for the browser; a screenshot and a test look at the same data.
import { readFileSync } from "node:fs";
import { richOf, bareOf, hostileOf, DOSSIER_MD } from "./aprender-scenarios.mjs";

const load = (name) => JSON.parse(readFileSync(new URL(`../fixtures/snapshot.${name}.json`, import.meta.url), "utf8"));

export { DOSSIER_MD };
export const richAprender = () => richOf(load("rich"));
export const bareAprender = () => bareOf(load("empty"));
export const hostileAprender = () => hostileOf(load("rich"));
