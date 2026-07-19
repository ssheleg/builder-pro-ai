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

## Protecting `main`

Three layers keep `main` release-quality; the first two are active now, the third is a paid/visibility opt-in.

1. **Release path (active, plan-independent).** `release.yml` refuses to run on any ref other than
   `main` (rule 4). So even if something lands on `main`, a *release* can only be cut from `main`.
2. **Local pre-push hook (active).** `scripts/git-hooks/pre-push` refuses a **force-push** or a
   **deletion** of `main` from this clone (the two irreversible mistakes). A normal fast-forward
   `nightbuild → main` push is allowed. Install per clone:

   ```bash
   bash scripts/setup-git-hooks.sh      # installs .git/hooks/pre-push
   ```

   (Hooks live per-clone under `.git/hooks`, so this runs once after cloning. The committed source
   is `scripts/git-hooks/pre-push`.)
3. **Server-side GitHub protection (opt-in).** Classic branch protection AND repository rulesets
   both require **GitHub Pro or a public repo** — on this private free-plan repo the API returns
   `403 "Upgrade to GitHub Pro or make this repository public"`. To enable it, either upgrade the
   plan or make the repo public (the signing `*.p12` files are gitignored, so nothing secret is in
   history — but review before publishing), then create the ruleset:

   ```bash
   gh api -X POST repos/ssheleg/builder-pro-ai/rulesets --input - <<'JSON'
   { "name": "main-protection", "target": "branch", "enforcement": "active",
     "conditions": { "ref_name": { "include": ["refs/heads/main"], "exclude": [] } },
     "rules": [
       { "type": "deletion" }, { "type": "non_fast_forward" },
       { "type": "pull_request", "parameters": { "required_approving_review_count": 0 } },
       { "type": "required_status_checks", "parameters": {
           "strict_required_status_checks_policy": true,
           "required_status_checks": [ {"context":"gates"}, {"context":"coverage"} ] } }
     ] }
   JSON
   ```

## Bootstrap note

This model was bootstrapped directly onto `main` (the QA-audit merge + this doc + the CI/script
wiring), because `nightbuild` did not exist yet. From that point on, **rule 1 applies** — the next
change goes through `nightbuild`.
