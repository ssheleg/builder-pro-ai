# Documentation-consistency fix plan — 2026-07-19

Found by a 4-auditor doc-vs-code sweep + a self-scan + re-measured test counts, at `[0.9.2]`
(HEAD `5e0c9ac`). Ground truth this pass: **Rust 1072 tests**, **TS 932 tests / 57 files**; released
version **0.9.2**; source version manifests still declare **0.1.0** (the root of the version drift).

Ordered by blast radius. Each row: file → fix. All executed in the same change as this plan.

## F1 — `docs/traceability.md` (frozen at [0.8.0])
- `:246` heading "…current (S-POLISH, `[0.8.0]`, 2026-07-17)" → `[0.9.2]`, 2026-07-19.
- `:248-249` Rust **1062 → 1072**.
- `:261-262` TS **870 / 51 files → 932 / 57 files**; refresh the delta narrative (name the 8 new
  frontend/diagnostics test files).
- Add a contract→test section for the post-[0.8.0] slices (S-UXR tokens/primitives/theme,
  S-DESIGN contrast, S-DIAG diag ring/ErrorBoundary/panel, BL-101 sync-manage) with their real,
  passing tests; scope the "Uncovered rows" completeness claim to cover them.
- Keep: `[1,1]` orchd claims + cited test names (verified still correct).

## F2 — `docs/build-macos.md`
- `:254-255` FACTUALLY WRONG: "CI is test-only / release runs `build-universal.sh` + `sign-verify.sh`
  locally". Reality: `release.yml` IS a CI (macos-15) workflow that produces signed+notarized
  artifacts and does NOT run `sign-verify.sh`. Fix: release path = `release.yml` (CI, `main`-only,
  signed/notarized/stapled); local `build-universal.sh` + `sign-verify.sh` = the manual alt path.
- `:357-358` example `version=0.8.0` (the abandoned Draft) → `0.9.2` / `<version>`.
- `:43-48` crate list omits `crates/orchd` + `crates/orchd-proto` → add them.
- `:360-362` note the release is `prerelease` by default too (not just draft).

## F3 — `CONTRIBUTING.md`
- `:45-48` fresh-checkout dev-setup builds/stages only `bpa-sessiond`; Tauri needs BOTH sidecars
  (`tauri.conf.json externalBin`) → build `bpa-sessiond` + `bpa-orchd` and stage both.
- `:83` "same set on every push/PR" → "on every push to `main` and every PR targeting `main`".

## F4 — `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md`
- Headline "Current slice" block (~`:216-227`) says shipped-through `[0.7.0]`; omits S-POLISH +
  0.9.x → shipped-through `[0.9.2]`, append S-POLISH + S-UXR/S-DIAG/S-DESIGN/BL-101.
- Roadmap table (~`:172-194`) + UI tenet (~`:44-45`): add the frontend-design-system + diagnostics
  slices so a shipped, fully-themed design system + a diagnostics layer are represented.
- Survival table orchd row (~`:110`) "no live runtime state, restart is a non-event" is stale since
  S-IDEA: an in-flight research run IS live state, lost on restart → `failed{interrupted}` (its own
  `:179` row already says so).
- Spec-reference list (~`:228-233`): add the S-POLISH + S-UXR spec files.
- Distribution note (~`:152`): signed+notarized binaries are now published on Releases.

## F5 — `README.md`
- Rust **1062 → 1072** (done this pass); TS **925 → 932**.

## F6 — `docs/runbook-orchd.md`
- `:76`, `:83`, `:116` "schema-v1" → **schema-v4** (`SCHEMA_VERSION = 4`; a reset DB lands at v4).
  `runbook-daemon.md` is clean.

## F7 — Version manifests (root cause; a real bug, not just docs)
- `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` declare `0.1.0`. `release.yml`
  uses its `version` input only for the tag/filename/title — it does NOT inject it into
  `tauri.conf.json`, so the built `[0.9.2]` app reports CFBundleShortVersion **0.1.0**. Bump all
  three to **0.9.2** so the binary reports its real version.

## Gate
`check-english.sh` + `cargo check -p builder-pro-ai` (config parse after the tauri.conf bump) +
commit + push. Frontend/rust behavior unchanged (docs + version metadata only).
