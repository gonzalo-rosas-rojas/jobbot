# 0008. Code that is not ours enters only through `check-lock.mjs`

Status: Accepted. Decided 2026-10-07 (spec section 10.1; the policy was written in R8 of the 2026-10-07 revision). Recorded 2026-10-10.

## Context

A package that reaches the page runs with the privileges of the panel (ADR 0002), and a package that only builds it runs on this machine at install time. The npm registry has had
compromises of popular packages, so "it is popular" is not a reason to trust a version.

## Decision

- Exact versions only, no ranges; `.npmrc` sets `save-exact`, `ignore-scripts`, `audit=false`, `fund=false` and `update-notifier=false`. Install with `npm ci --ignore-scripts`.
- `check-lock.mjs` holds `EXPECTED`, a second source for every package: version, licence, publication date and sha512 taken from the registry with a separate query, not from the
  lockfile. It fails when the lock disagrees, when a package nobody approved appears, when a package resolves from anywhere but `registry.npmjs.org`, when any package but esbuild has an
  install script, when a licence is outside the allow-list (MIT, ISC, BSD, 0BSD, Unlicense, CC0, OFL-1.1, Apache-2.0), or when a version was **less than 7 days old** when it entered
  the project (the quarantine; npm 11.1 has no `min-release-age`).
- Each package that reaches the page has a row in `THIRD_PARTY_NOTICES.md`; fonts and icons come only through `tools/fonts.mjs` and `tools/icons.mjs` and are listed in `VENDORED.md`
  with their hashes. `node tools/audit-deps.mjs` greps what each dependency does with the network, strings-as-code and markup.
- Adding or raising a package is a commit of its own, never mixed with a change of behaviour. Agents working on moments do not add packages.

## Consequences

- Slow on purpose: a new tool for development (a type checker, a property-testing library) is a deliberate step with a row to write, a quarantine to wait out and a licence to check.
  The scripts of this folder (the layer test, the checks) therefore use no dependency beyond esbuild and Node.
- A licence outside the list (for instance the MPL-2.0 of axe-core) needs a decision to widen the list.
- Dropping what the page does not use is part of the policy, not a favour: `three`, `animejs` and `simplex-noise` stayed in `package.json` and in `EXPECTED` after the tower was archived,
  and the question of removing them with the foundations lab was decision D6 of the architecture review. Gonza approved it and it was done on 2026-10-10: the lab went in `baaebaf`
  (what still mattered moved: the fictional postings to `test/fixtures/fake.mjs`), and the three packages left `package.json`, the lock, `EXPECTED`, `tools/audit-deps.mjs`,
  `tools/weigh.mjs` and the notices in `1a02e72`; the built page was byte-identical before and after. `guard.mjs` still refuses a bundle that includes `node_modules/three`, so the
  tower cannot come back through a stray import.
