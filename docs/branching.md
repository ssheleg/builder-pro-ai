# Branch model & build flow

Two long-lived branches. **Work happens on `nightbuild`; `main` is stable and release-only.**

## `main` — stable / release-only

- Every commit on `main` is release-quality. `main` tracks `origin/main`.
- `main` receives changes **only** by merging `nightbuild` (ideally via a PR) — never direct
  feature work.
- GitHub CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) runs the full gate on every
  push to `main` and every PR **targeting** `main`.
- Release builds are cut **only from `main`**: [`release.yml`](../.github/workflows/release.yml) is
  manual (`workflow_dispatch`) and its first step **refuses to run on any ref other than `main`**.

## `nightbuild` — working / integration branch

- The default day-to-day branch. All ongoing work lands here.
- Feature branches and git worktrees branch **off** `nightbuild` and merge **back into**
  `nightbuild` — **never into `main`**.
- Validated **locally** with `bash scripts/final-suite.sh`. GitHub does **not** auto-build the
  working branch (keeps the 10×-billed macOS runners for the release path only).
- Local **test builds** are produced from `nightbuild` (whatever you're working on):

  ```bash
  npm run build:test      # unsigned, host-arch, debug .app — for local testing only
  ```

## Flow

```
feature / worktree ──merge──▶ nightbuild ──(local test builds + final-suite)──▶ PR ──▶ main ──(manual release.yml, main-only)──▶ signed universal DMG
```

- **Local, from `nightbuild`:** iterate → `npm run build:test` → `bash scripts/final-suite.sh`.
- **GitHub, from `main`:** merge `nightbuild → main` (CI gate runs on the PR) → later, an owner
  manually dispatches `release.yml` from `main` → universal, signed, notarized DMG.

## Rules (normative)

1. **Never commit feature work directly to `main`.** Work on `nightbuild` (or a short branch off it).
2. **Worktrees** ([`superpowers:using-git-worktrees`](../.superpowers)) start from `nightbuild`, and
   their `finishing-a-development-branch` step merges into **`nightbuild`, not `main`**.
3. `main` advances **only** by merging `nightbuild` after `scripts/final-suite.sh` is green locally
   and CI is green on the PR.
4. **Release (`release.yml`) runs only from `main`** — enforced by a ref guard in the workflow.
5. **Local test build** = `npm run build:test` (unsigned, host-arch, debug — Gatekeeper will reject
   it elsewhere, by design). **Release build** = universal + signed + notarized, from `main` only
   (`scripts/build-universal.sh` via `release.yml`).

## Recommended branch protection (set once on GitHub)

These make the rules mechanical rather than by-convention (configure under Settings → Branches):

- Protect `main`: require a PR, require the `ci / gates` check to pass, disallow direct pushes.
- Optionally protect `nightbuild`: require the `ci` check on PRs into it (only if you opt into
  running CI for feature→nightbuild PRs; by default the local `final-suite` is the gate there).

## Bootstrap note

This model was bootstrapped directly onto `main` (the QA-audit merge + this doc + the CI/script
wiring), because `nightbuild` did not exist yet. From that point on, **rule 1 applies** — the next
change goes through `nightbuild`.
