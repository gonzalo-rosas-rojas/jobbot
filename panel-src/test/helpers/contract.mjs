// The snapshot keys the page may rely on. One list for the fixtures, the demo and (through include_str!) the Rust test.
import { readFileSync } from "node:fs";

export const KEYS = JSON.parse(readFileSync(new URL("../fixtures/snapshot-keys.json", import.meta.url), "utf8"));
export const ACCOUNT_KEYS = ["ok", "message", "subscription", "email", "checked_at", "auth_method", "api_provider", "org_id", "config_dir", "cli_version", "anchored"];
export const TASK_KEYS = ["exists", "next_run_at"];

/** True when `path` ("a.b[].c") exists in `obj`. `[]` means "in at least one element". A key that holds null exists. */
export function has(obj, path) {
  const step = (node, parts) => {
    if (parts.length === 0) return true;
    const [head, ...rest] = parts;
    if (head.endsWith("[]")) {
      const list = node == null ? undefined : node[head.slice(0, -2)];
      return Array.isArray(list) && list.some((item) => step(item, rest));
    }
    return node != null && typeof node === "object" && Object.hasOwn(node, head) && step(node[head], rest);
  };
  return step(obj, path.split("."));
}

/** The paths of the contract the snapshot lacks. `withCards` adds the paths that need a card, task, mail or move to exist; `whenAccount` the ones inside `controls.claude_account`, which is null until the engine has checked the login; `whenTrust` the ones inside `trust`, which is null while criteria.toml cannot be read. A section listed in `optional` must be there, and may be null (the server could not build it): then the paths inside it are not asked for. */
export function missing(snapshot, { withCards = true } = {}) {
  const account = snapshot?.controls?.claude_account != null;
  const trust = snapshot?.trust != null;   // null when criteria.toml cannot be read
  const absent = KEYS.optional.filter((section) => !has(snapshot, section));
  const empty = new Set(KEYS.optional.filter((section) => snapshot != null && Object.hasOwn(snapshot, section) && snapshot[section] === null));
  const inside = (p) => empty.has(p.split(".")[0].replace(/\[\]$/, ""));
  return [...absent, ...[...KEYS.always, ...(withCards ? KEYS.withCards : []), ...(account ? KEYS.whenAccount : []), ...(trust ? KEYS.whenTrust : [])].filter((p) => !inside(p) && !has(snapshot, p))];
}
