# AGENTS.md — cannify_micro_template

Guidance for agents and contributors. [FEATURES.md](FEATURES.md) is the only status tracker (areas FW, API, HOST, PY,
TPL with stable IDs; update it in the same commit as the code, never renumber), [CHANGELOG.md](CHANGELOG.md) records
changes under `[Unreleased]` until a release. [README.md](README.md) explains the project to a user.

## Layout

The repo is a Cargo workspace of host crates plus the firmware as a standalone crate (`firmware/`, excluded from the
workspace: it builds for `thumbv6m-none-eabi` with its own `.cargo/config.toml` and `Cargo.lock`):

- `cannify_micro_api/` — the API trait, shared by the firmware and the hosts (a workspace member and the firmware's
  path dependency).
- `cannify_micro_client/` — the library: the generated WireWeaver client (`CannifyClient`) and the typed `Cannify` on
  top with one `Error`. Everything that talks to the board lives here; the CLI and the Python module only parse and
  print.
- `cannify_micro_cli/` — `cannify` (clap, anstream colour, dynamic completion with live probe serials, `--version`
  with git SHA and build time from `build.rs`).
- `cannify_micro_py/` — the `cannify_micro` Python module (PyO3, maturin; `uv` drives it: `just py`, `just test-py`).
- `scripts/rename.py` — `just rename NAME [SHORT]`, run once by a user of the template.
- `docs/board/` — the board map: `b129a_cannify.toml` (what to draw, what every pin does) and the geometry JSON
  extracted from ST's board file; the SVGs and `b129a_cannify-pins.md` are rendered from them by `just board`
  (boardmap from embedded_bedrock) and committed, so GitHub shows them. Edit the TOML, never the pictures.
- `docs/logo/` — the logo: `template_logo.typ` (the same for every template repo), `logo.typ` (this board's name, chip
  label and subtitle), `pair.svg` (WireWeaver's twisted pair, fixed artwork), JetBrains Mono Bold (printable ASCII
  subset) in `fonts/`; `just logo` renders `logo.svg`, `logo-dark.svg` and `social-preview.png`, committed. Edit the
  `.typ`, never the pictures.

WireWeaver and embedded_bedrock crates come from git, pinned by the lock files; a change there is made upstream,
not worked around here.

## Commands

```sh
just test          # host tests
just lint          # fmt check, clippy (host and firmware), release firmware build
just run           # dev firmware with its defmt log
just flash         # release firmware through the probe
just install       # cannify on this PC
just py / just py-example / just test-py
```

## Conventions

- Adding to the API: trait first (`cannify_micro_api`), then the firmware (`firmware/src/ww.rs`), then a typed
  method in the client, then the CLI and Python. Bump the API crate's version: `just api-check` (also in `just test`,
  CI and `.githooks/pre-commit`) compares it with `cannify_micro_api/api_snapshots/` and names the bump a change needs;
  `just api-save` saves (or refreshes) the current version's snapshot. Never edit or re-save a committed snapshot
  of a released version to make the check pass; bump the version instead.
- Conventional Commits with a scope: `feat(fw): ...`, `feat(client): ...`, `feat(cli): ...`, `feat(py): ...`,
  `docs: ...`, `build: ...`; one CHANGELOG entry per change a user notices.
- Public wording: the README, docs and recipe comments say what happens in neutral words.

The rest of this file is the firmware template's own guidance, generated with the firmware.

---

# AGENTS.md — cannify_micro

This firmware (STM32G0B1CE, `embassy`) was generated from the **embedded_bedrock firmware template**
(`firmware_template_skill/` in https://github.com/romixlab/embedded_bedrock, the `firmware-template` agent skill, generator
`scripts/bedrock_gen.py`). After generation it is an ordinary Rust project and is meant to be edited freely.

## Layout: WireWeaver API crate + firmware

The firmware serves a [WireWeaver](https://github.com/vhrdtech/wire_weaver) API over USB. The root `Cargo.toml` is a
Cargo workspace of the host side (with its `Cargo.lock`, which `#[ww_api_root]` needs to find the API's git
dependencies); the firmware is a standalone crate next to it, excluded from the workspace because it builds for
`thumbv6m-none-eabi` with its own `.cargo/config.toml` and `Cargo.lock`:

- `cannify_micro_api/` — `#![no_std]` API crate, a workspace member and the firmware's path dependency: the
  `CannifyMicroApi` trait (`#[ww_api_root]`) and its data types (`#[derive_shrink_wrap]`).
  Its name + version identify the API on the wire: bump the version with every change and follow WireWeaver's
  `docs/evolution/rules.md` so deployed devices and hosts keep working together.
- `firmware/` — the firmware; `src/ww.rs` implements the API on `ServerState` (`ww_codegen!`) and runs the transport.
  All firmware commands (`cargo build`, `cargo run`) run there.

WireWeaver crates come from git (`https://github.com/vhrdtech/wire_weaver`, pinned by `firmware/Cargo.lock` and the root's).
Its `AGENTS.md` and `docs/` explain the API DSL (methods, properties, streams) and the wire format.

The trait, `ServerState` and the method implementations were blinky stubs; after generation they are this
firmware's own code. On template upgrades take only plumbing changes there (dependencies, transport set-up,
`ww_codegen!` arguments), never the stubs.

`bedrock_fw.json` (project root, keep it under version control) records:

| key | meaning |
|---|---|
| `template` | repo, sub-directory, **git commit** and CHANGELOG version of the template this firmware was last generated / upgraded from |
| `answers` | every generator option (chip, framework, logging, bootloader, counters, ...) — regenerating with them reproduces the project |
| `command` | the same answers as a `bedrock_gen.py new ...` command line (informational) |
| `upgrades` | history of applied template upgrades |
| `rejected` | template changes the user declined; **do not propose them again** unless upstream changed them further |
| `nuances` | firmware-specific facts that matter for upgrades (e.g. "clock tree hand-written, never take main.rs clock init") |

Do not edit `template` by hand except as part of an upgrade (below). Keep `answers` in sync if you change something
the generator decides (e.g. switch logging, add the bootloader) so later upgrades regenerate the right thing.

## Updating from the template

The firmware can later pick up template improvements (crate bumps, bug fixes, new features). When the user asks for
a template update/upgrade, **first produce a report, change nothing** until the user approves.

1. **Get the template.** Clone it into a temp dir, `git clone https://github.com/romixlab/embedded_bedrock "$TMP/tpl"` (or use a local
   checkout / the installed `firmware-template` skill if the user points to one; do not modify that checkout).
   Target revision: latest `main` unless the user names one. `S="$TMP/tpl/<template.path>/scripts/bedrock_gen.py"`;
   run it with `uv run "$S" ...`.
2. **What changed.** `OLD=<template.commit>`:
   `git -C "$TMP/tpl" log --reverse --format='%h %ad %s%n%b' --date=short $OLD..HEAD -- <template.path>` and the entries
   of `<template.path>/CHANGELOG.md` newer than `template.version` — read their *Upgrade notes*. Use
   `git -C "$TMP/tpl" diff $OLD HEAD -- <template.path>/scripts` for details. If `OLD` is null/unknown, skip the base
   (step 4) and rely on the changelog.
3. **New options.** `uv run "$S" check-answers bedrock_fw.json` lists options that did not exist when this firmware
   was generated (and removed ones). Ask the user about each `NEW` option (explain it, suggest the default) and about
   any changelog entry that changes a default or behaviour the firmware relies on. Record the answers.
4. **Regenerate** into temp dirs with the same answers (`--answers` reuses them, explicit flags override / add new
   options):
   - new: `uv run "$S" new --answers bedrock_fw.json [--new-option ...] --out "$TMP/new"`
   - base (optional but strongly recommended, shows what the template changed vs what the firmware changed):
     `git -C "$TMP/tpl" worktree add --detach "$TMP/old" $OLD` then
     `uv run "$TMP/old/<template.path>/scripts/bedrock_gen.py" new --answers bedrock_fw.json --out "$TMP/base"`
5. **Fuzzy compare.** `uv run "$S" compare --current . --new "$TMP/new" [--base "$TMP/base"] [--diff]` classifies
   files (whitespace-insensitive): `take-new` (template changed, file untouched locally), `merge` (both changed),
   `added-upstream`, `removed-upstream`, `deleted-locally`, `up-to-date`. Then read the diffs yourself: the firmware
   was edited by humans, so match template changes to the corresponding code by meaning (renamed functions, moved
   blocks, reformatted `Cargo.toml`), not by line. Ignore noise that is not an upgrade: generated nightly date,
   `bedrock_fw.json`, `Cargo.lock`, stm32-data differences unrelated to the change.
6. **Report** to the user, grouped per logical upgrade (not per file), each with: id/short title, source (commits,
   changelog entry), what it changes in *this* firmware, files, risk / manual work (merge conflicts, API changes,
   needs hardware test), recommendation. List separately: changes skipped because they are in `rejected` (only
   mention them if upstream changed them again), changes that do not apply, new options and what was answered.
   Ask which upgrades to apply.
7. **Apply only the approved upgrades**, preserving local modifications; merge by hand where needed. Then
   `cargo build` in `firmware/` (and in `firmware/bootloader/` if present) — fix until it builds; mention anything
   that needs testing on hardware.
8. **Update `bedrock_fw.json`**: set `template` to the new commit/version/date (copy it from
   `$TMP/new/bedrock_fw.json`), update `answers` (new options, changed answers) and `command` likewise, append to
   `upgrades` `{"date", "from", "to", "applied": [...], "notes"}`, add declined items to `rejected`
   `{"id", "title", "commit"/"version", "files", "reason", "date"}`, and add anything learned about this firmware to
   `nuances`. Even when everything was rejected, record the new commit so the next upgrade starts from it.
9. Remove the temp dirs / worktree (`git -C "$TMP/tpl" worktree remove "$TMP/old"`).

If only some upgrades were applied, the firmware is still recorded at the new commit; the rejected list is what keeps
the skipped changes from being proposed again.
