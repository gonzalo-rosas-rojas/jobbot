# 0005. Private state under one root, `JOBBOT_HOME`

Status: accepted

## Context

jobbot holds a person's data: the answers typed into forms, the CV, the database of applications,
and the login of the Claude account it calls. All of it lived inside the project folder. The
`claude -p` child also worked in a folder inside the repository, where a `CLAUDE.md` file above it
could reach the model.

## Decision

- All private state sits under one root: the profile (`profile/*.toml`), the CV master, `data/`
  (database, CVs, screenshots, dossiers, digests) and the `STOP` file. By default the root is the
  project folder, where the running installation already keeps it; the variable `JOBBOT_HOME`
  names another folder, and then every command, the panel included, reads all of it from there.
- A `JOBBOT_HOME` without a `profile/` folder stops the run with its own message; it never falls
  back to the project folder.
- Claude's config folder and working folder are under `%LOCALAPPDATA%\jobbot`, outside the
  repository, whatever the root (Windows always sets that variable; without it the working folder
  falls back to `data/claude-cwd`). The child also runs with `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`.

## Consequences

- With the variable set, a `STOP` file left in the project folder is not read: there is one kill
  switch, in one place.
- In the default setup the real profile and CV are tracked in the private repository and only
  `data/`, `STOP` and `profile/*.local.toml` are git-ignored, so that repository and its history
  are never published. A public copy is made by `cargo xtask export-public`: one commit, a
  whitelist that never takes `profile/`, `cv/` or `data/`, the example profile in their place, a
  scan, and a fresh history.
- Moving Claude's working folder changes where a one-shot call runs; such calls keep no state in it.
