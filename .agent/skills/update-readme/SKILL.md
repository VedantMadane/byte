---
name: update-readme
description: "Use when README.md may be stale. Discovers commits since the last README update, identifies what user-facing surfaces changed, and brings README.md back into sync."
---

# Updating the README

**Governing spec sections:** §3 (`README.md` — required sections and content), §21.5 (this skill is mandated because `README.md` is a drift-prone artifact).

`README.md` is the primary user-facing documentation for byte. Per §3 of `OSS_SPEC.md` it must cover the project description, installation, a quick-start, usage, contribution pointer, license, and a link to `OSS_SPEC.md`. It goes stale whenever a CLI flag, subcommand, default, or supported surface changes without a matching edit.

## Tracking mechanism

`.agent/skills/update-readme/.last-updated` contains the git commit hash from the last successful run. Empty means "never run" — fall back to the initial commit of the repository.

## Discovery process

1. Read the baseline:

   ```sh
   BASELINE=$(cat .agent/skills/update-readme/.last-updated)
   ```

2. List commits since the baseline:

   ```sh
   git log --oneline "$BASELINE"..HEAD
   ```

3. List changed files:

   ```sh
   git diff --name-only "$BASELINE"..HEAD
   ```

4. Categorize the changes using the mapping table below.

5. Read the current `README.md` so you can preserve voice and unrelated sections while editing.

## Mapping table

| Changed files / scope | README section(s) to update |
|---|---|
| Public API surface | **Usage** / **Quick start** |
| CLI flags or subcommands | **Usage** table |
| Default configuration | **Quick start** |
| Installation instructions / package name | **Install** |
| New supported platform or language | **Supported platforms** list |
| License change | **License** section, badges |
| `src/cli/mod.rs` (the clap `Command` enum and its `#[arg]` attributes) | **Usage** table — this is the authoritative list of subcommands and flags; diff it against the table row by row |
| `src/store/secrets.rs`, `src/store/metadata.rs` (what is stored where) | **Why?** bullets, **Configuration** pointer |
| `SECURITY.md` (claims about what sits in plaintext on disk) | **Why?** bullets — the README summarizes these and drifts when the inventory grows |

Extend this table every time you find a new source-of-truth file that feeds the README.

## Update checklist

- [ ] Read baseline from `.last-updated` and run `git log` / `git diff --name-only`
- [ ] Read the current `README.md`
- [ ] Walk the mapping table and update each affected section
- [ ] Verify every shell example is still syntactically valid
- [ ] Run `make test` and the project's own conformance check
- [ ] Write the new baseline:

      git rev-parse HEAD > .agent/skills/update-readme/.last-updated

## Verification

1. Re-read every edited section against the corresponding source of truth.
2. Confirm `.last-updated` was rewritten with the new `HEAD`.

## Skill self-improvement

After a run, improve this file in place:

1. **Grow the mapping table** with any new source → README relationship you discovered.
2. **Record patterns** for recurring edits.

   Patterns found so far:

   - `README.md` is **CRLF** in the working tree. Any scripted edit must
     preserve that, or the diff becomes a whole-file rewrite that buries the
     real change. Normalize the CR-LF pairs to bare LF before matching,
     then restore CR-LF on write, and confirm with `git diff --stat` that
     the insertion count matches the lines you actually meant to touch.
   - A storage-layer refactor can leave every CLI surface accurate while
     still making a **Why?** bullet wrong. Check the "what lives where"
     claims, not just the command table, whenever `src/store/` changes.
   - `make` is not available on the maintainer's Windows box; `cargo test`
     is the equivalent of `make test` (see the `Makefile`). `oss-spec
     validate` aborts before reporting when no agent CLI is on `PATH` —
     that is an environment limitation, not a conformance failure, and must
     be reported as such rather than glossed as "spec check passed".
3. **Commit the skill edit** together with the README edit so the knowledge compounds.