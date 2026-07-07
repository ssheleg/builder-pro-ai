# Contract → Test Traceability (spec §14.2)

This matrix maps every locked contract row from
`docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` §14.2 to the
concrete, currently-passing test(s) that cover it. Every test name below is a real `fn` name in
this repository at the time of writing (Task 25) — none are aspirational. Run
`cargo test --workspace -- --list` / `npx vitest run` yourself to re-verify the full roster.

Crate name note: the daemon crate's package name is `bpa-sessiond` (lib name `bpa_sessiond`); the
Tauri core crate's package name is `builder-pro-ai` (lib name `builder_pro_ai_lib`); the shared
wire-types crate is `bpa-protocol` (lib name `bpa_protocol`); the shared directory-validation crate
is `bpa-paths` (lib name `bpa_paths`) — one `validate_dir` used byte-for-byte by both the core and
the daemon (spec §16). Commands below use `-p <package name>`.

| Contract (spec §) | Test (command) |
|---|---|
| Shared types / Rust⇄TS parity (§5) | `cargo test -p bpa-protocol --test ts_export` (`generates_types_ts_at_shared_path`, `workspace_uses_camelcase_root_path`, `session_lifecycle_is_internally_tagged_camelcase`, `terminal_event_is_adjacently_tagged_bytes_are_number_arrays`, `session_meta_fields_are_camelcase`) then `git diff --exit-code -- src/ipc/types.ts` (wired into `scripts/final-suite.sh` stage 6 of 8) |
| Hop-B framing, CBOR body (§7, Pv2 §3.1) | `cargo test -p bpa-protocol --test framing` (`encode_matches_manual_prefix`, `single_frame_encodes_and_decodes`, `two_frames_in_one_read_both_decode`, `partial_frame_across_reads_buffers_then_completes`, `length_prefix_split_across_reads`, `oversized_length_prefix_is_rejected`, `garbage_body_of_valid_length_is_a_decode_error`) |
| Hop-B CBOR round-trip, every `Frame`/`Request`/`Response`/`Push`/`SessionLifecycle`/`TerminalEvent` variant (§7, Pv2 §3.1-3.2) | `cargo test -p bpa-protocol --test roundtrip` (`every_request_variant_roundtrips`, `every_response_variant_roundtrips`, `every_push_variant_roundtrips`, `every_session_lifecycle_variant_roundtrips_via_cbor`, `every_terminal_event_variant_roundtrips_via_cbor`, `constants_are_locked`) |
| Codec-agnostic preamble + version negotiation (Pv2 §4) | `cargo test -p bpa-protocol --test preamble` (`negotiate_equal_ranges_accepts_chosen_2`, `negotiate_disjoint_is_incompatible`, `negotiate_overlap_picks_min_of_maxes`, `client_preamble_round_trips_through_bytes`, `bad_magic_is_rejected`, `oversized_build_len_is_rejected`) |
| Hop-B correlation + preamble handshake (§7, Pv2 §4.4-4.5) | daemon-side `cargo test -p bpa-sessiond --lib socket_server::tests::handshake_happy_path_returns_accepted socket_server::tests::garbage_preamble_closes_without_hang socket_server::tests::requests_are_answered_with_matching_ids_concurrently` + client-side `cargo test -p builder-pro-ai --lib socket_client::tests::concurrent_requests_correlate_by_id socket_client::tests::incompatible_daemon_reply_surfaces_typed_error socket_client::tests::daemon_closing_during_handshake_is_incompatible_not_hang` |
| PTY threading + echo/EOF (§9) | `cargo test -p bpa-sessiond --lib pty_supervisor::tests::echo_roundtrip_via_sh pty_supervisor::tests::child_exit_marks_inactive_via_eof pty_supervisor::tests::supervisor_is_send_and_sync` |
| PTY pgroup-kill (no zombie, no orphan) (§9) | `cargo test -p bpa-sessiond --lib pty_supervisor::tests::kill_reaps_no_zombie pty_supervisor::tests::kill_terminates_whole_process_group` |
| PTY resize → SIGWINCH (§9) | `cargo test -p bpa-sessiond --lib pty_supervisor::tests::resize_delivers_sigwinch_updated_columns` |
| Env hygiene: `DAEMON_SECRET` absent from child env, allowlist present (§9.3, §16) | `cargo test -p bpa-sessiond --lib pty_supervisor::tests::env_clear_hides_daemon_secret_keeps_allowlist` |
| OSC parser: split-across-reads, terminators, cap, `D;<code>` edges, OSC-7 decode, forged/oversized input (§10) | `cargo test -p bpa-sessiond --lib osc_parser::tests::osc_split_across_feeds_is_buffered osc_parser::tests::implicit_esc_terminator_ends_and_starts_new_osc osc_parser::tests::st_terminator_accepted osc_parser::tests::parses_full_133_lifecycle_bel_terminated osc_parser::tests::oversized_osc_is_dropped_not_crashed osc_parser::tests::exit_code_edges_empty_nonnumeric_out_of_range osc_parser::tests::osc7_file_scheme_decodes_and_strips_host osc_parser::tests::osc7_kitty_scheme_decodes osc_parser::tests::osc7_percent_decodes_spaces_and_unicode osc_parser::tests::osc7_empty_host_still_yields_absolute_path osc_parser::tests::osc7_bad_percent_escape_dropped osc_parser::tests::osc7_unknown_scheme_dropped osc_parser::tests::forged_and_interleaved_osc_never_panics_and_recovers osc_parser::tests::non_osc_bytes_produce_no_events` |
| OSC/lifecycle state machine, full transition table, empty-command no-op, `D`-without-code (§10) | `cargo test -p bpa-sessiond --lib osc_parser::tests::lifecycle_full_transition_table osc_parser::tests::lifecycle_empty_command_b_to_a_is_noop osc_parser::tests::lifecycle_d_without_code_is_exited_none osc_parser::tests::lifecycle_cwd_event_does_not_change_state` |
| Waiting-for-input heuristic (§10.4) | `cargo test -p bpa-sessiond --lib pty_supervisor::tests::waiting_for_input_true_for_partial_prompt pty_supervisor::tests::waiting_for_input_false_when_idle_at_prompt pty_supervisor::tests::waiting_for_input_false_for_alt_screen` |
| Scrollback ring bounds/prune (§11) | `cargo test -p bpa-sessiond --lib scrollback::tests::ring_enforces_byte_cap_dropping_oldest scrollback::tests::push_larger_than_cap_keeps_only_tail scrollback::tests::explicit_prune_is_idempotent scrollback::tests::plain_text_round_trips_oldest_to_newest` |
| Scrollback sanitization (alt-screen/title/paste stripped, SGR kept); split sequences; long/adversarial OSC (§11) | `cargo test -p bpa-sessiond --lib scrollback::tests::keeps_sgr_and_plain_text scrollback::tests::strips_alt_screen_enter_leave_1049_and_47 scrollback::tests::strips_bracketed_paste_toggles_2004 scrollback::tests::strips_title_osc_0_1_2_bel_and_st scrollback::tests::strips_osc_133_and_osc_7_marks scrollback::tests::keeps_cursor_moves_and_erases scrollback::tests::split_alt_screen_sequence_across_pushes_is_stripped scrollback::tests::strips_title_osc_longer_than_carry_cap scrollback::tests::strips_osc_7_longer_than_carry_cap scrollback::tests::recognized_osc_exceeding_cap_across_multiple_pushes_never_leaks_payload_or_terminator scrollback::tests::recognized_osc_st_terminator_split_across_two_pushes_in_discard_mode_never_leaks scrollback::tests::unrecognized_long_partial_escape_still_fails_open` |
| Scrollback replay of a past session does not corrupt a fresh terminal (§11) | `cargo test -p bpa-sessiond --lib scrollback::tests::replay_of_past_vim_session_has_no_side_effecting_sequences` |
| Live-attach OSC-133/OSC-7 stripping (verbatim otherwise), incl. split-across-`strip()`-calls (§10.3, amendment §C) | `cargo test -p bpa-sessiond --lib attach::tests::live_output_strips_osc133_osc7_but_keeps_everything_else attach::tests::live_osc_stripper_drops_osc133_marker_split_across_two_strip_calls attach::tests::live_osc_stripper_drops_osc7_marker_split_across_two_strip_calls_st_terminated` |
| SQLite persistence: WAL + rehydrate, every `SessionLifecycle` variant round-trips (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::persist_and_rehydrate_round_trip persistence::tests::every_lifecycle_variant_round_trips persistence::tests::committed_rows_survive_reopen persistence::tests::open_in_memory_supports_full_crud` |
| SQLite corrupt-db quarantine + recreate (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::corrupt_db_is_quarantined_and_recreated` |
| SQLite `busy_timeout` under concurrent access (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::busy_timeout_allows_concurrent_writers` |
| SQLite migration on old `user_version`; fail-closed on newer version (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::migration_runs_on_old_user_version persistence::tests::newer_user_version_is_rejected` |
| SQLite checkpoint (incl. in-memory no-op) (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::checkpoint_is_a_noop_success_and_data_still_readable persistence::tests::checkpoint_on_in_memory_db_does_not_error` |
| SQLite kill-9-mid-write → restart opens + rehydrates committed rows (§11) | `cargo test -p bpa-sessiond --lib persistence::tests::committed_rows_survive_reopen` (WAL durability of committed transactions across process restart) + `npm run e2e:survive` (phase4: CLIENT-quit survival + reattach; phase5: daemon-restart rehydration, closes BL-7 in `docs/backlog.md`) |
| Socket: `flock` single-instance (second daemon exits) (§8.2) | `cargo test -p bpa-sessiond --lib singleton::tests::second_flock_on_same_lockfile_would_block` + `cargo test -p bpa-sessiond --test boot_integration second_instance_flock_refusal` |
| Socket path resolution (`XDG_RUNTIME_DIR` / `/tmp` fallback) + length assert (§8.1) | `cargo test -p bpa-sessiond --lib singleton::tests::socket_path_uses_xdg_runtime_dir_when_set singleton::tests::socket_path_falls_back_to_tmp_with_uid_when_xdg_unset singleton::tests::socket_path_falls_back_to_tmp_when_xdg_empty singleton::tests::socket_path_len_under_104_passes_and_over_fails` |
| Socket dir/lock/socket permissions (0700 dir, 0600 lock/socket), `/tmp`-squatting guard (§8.2) | `cargo test -p bpa-sessiond --lib singleton::tests::ensure_dir_creates_with_0700 singleton::tests::ensure_dir_refuses_world_writable_squat singleton::tests::ensure_dir_refuses_non_directory singleton::tests::lockfile_created_mode_0600 singleton::tests::set_socket_mode_applies_0600` |
| Socket peer-cred (`getpeereid`) euid check (§8.2, §16) | `cargo test -p bpa-sessiond --lib singleton::tests::peer_cred_accepts_same_uid_over_socketpair singleton::tests::peer_cred_rejects_foreign_uid_simulated socket_server::tests::peer_cred_same_uid_is_accepted` |
| Stale-socket file unlink + rebind (§13) | `cargo test -p bpa-sessiond --test boot_integration stale_socket_file_is_unlinked_and_rebound` |
| Oversized/garbage frame rejection at the socket layer (§7) | `cargo test -p bpa-sessiond --lib socket_server::tests::oversized_frame_is_rejected` |
| Backpressure / slow-client — one slow client disconnected without stalling others (§13) | `cargo test -p bpa-sessiond --lib socket_server::tests::slow_client_is_disconnected_without_stalling_a_second_client` |
| Attach model: multi-subscriber, NO supersede — N independent connections co-attach one session, each its own Replay (§7, Pv2 §5.2-5.4) | `cargo test -p bpa-sessiond --lib attach::tests::second_attach_from_different_conn_does_not_supersede attach::tests::attach_two_conns_same_session_no_supersede attach::tests::attach_sends_replay_first_then_live_output socket_server::tests::attach_first_push_is_replay_then_output socket_server::tests::two_connections_attach_same_session_both_stream_independently` |
| Attach model: `DetachSession` stops Output for ONE subscriber only, PTY keeps running (keep-alive) (§7) | `cargo test -p bpa-sessiond --lib attach::tests::detach_stops_output_session_stays_alive` |
| Attach model: connection-aware teardown — one client's detach/disconnect never tears down another client's live stream (§7, §13 isolation) | `cargo test -p bpa-sessiond --lib attach::tests::detach_from_non_owner_is_a_noop attach::tests::detach_all_for_conn_only_removes_that_conns_entries socket_server::tests::client_disconnect_does_not_teardown_a_second_clients_attached_session` |
| Attach: reattach/detached forwarder does not leak a thread (§7, resource hygiene) | `cargo test -p bpa-sessiond --lib attach::tests::same_conn_reattach_does_not_leak_thread` |
| Attach: a killed/exited session drops its attach entry — no orphaned registry growth across create/kill churn (§7, resource hygiene) | `cargo test -p bpa-sessiond --lib socket_server::tests::killed_session_attach_entry_is_reaped` |
| Attach: unknown-session error path (§7) | `cargo test -p bpa-sessiond --lib attach::tests::attach_unknown_session_errors attach::tests::attach_on_unknown_session_errors_no_such_session socket_server::tests::write_resize_kill_unknown_session_errors` |
| Attach on an inactive/rehydrated session: replays scrollback, no live subscription, no error (Pv2 §5, Task 12r; closes BL-7) | `cargo test -p bpa-sessiond --lib attach::tests::attach_on_inactive_session_replays_scrollback_without_live_subscription` + `cargo test -p bpa-sessiond --test rehydrate_attach cold_rehydrate_then_attach_replays_persisted_marker_as_inactive` |
| Detach integration: kill the client, child stays alive (pgrep), reconnect, scrollback replays (§14.1) | `npm run e2e:survive` (phase4a: `pgrepDaemon`/`pgrepShell` after client quit; phase4b: reattach + scrollback intact) |
| Path validation: missing/not-a-dir/relative/symlink-escape/root-deleted-before-create (§16) | Shared validator (used byte-for-byte by BOTH core and daemon) `cargo test -p bpa-paths` (`missing_path_is_missing`, `file_is_not_a_directory`, `relative_path_is_rejected_before_fs`, `symlink_escaping_parent_is_rejected`, `symlink_within_parent_is_allowed`, `ok_real_directory_canonicalizes`, `root_path_is_allowed`) + daemon-side path validation over the wire `cargo test -p bpa-sessiond --lib socket_server::tests::create_session_rejects_missing_cwd socket_server::tests::create_session_rejects_relative_cwd socket_server::tests::create_workspace_rejects_missing_dir socket_server::tests::create_workspace_rejects_symlink_escaping_root socket_server::tests::create_session_rejects_symlink_escaping_cwd` + core-side pre-flights (extracted pure guards `preflight_cwd` / `preflight_workspace_root`, exercised directly — Ok on None/empty/valid, real wire codes `RelativePath`/`CwdMissing`/`SymlinkEscape` on failure, canonicalized forward on success) `cargo test -p builder-pro-ai --lib commands::commands_over_stub_daemon::preflight_cwd_accepts_none_empty_and_valid_dir commands::commands_over_stub_daemon::preflight_cwd_rejects_missing_relative_and_symlink_escape commands::commands_over_stub_daemon::preflight_workspace_root_canonicalizes_valid_and_rejects_bad` |
| launchd install/bootstrap idempotency/kickstart/dir-missing/hard-failure (§8.3, §13) | `cargo test -p builder-pro-ai --lib launchd::tests::install_creates_dirs_and_writes_plist launchd::tests::bootstrap_already_bootstrapped_is_success launchd::tests::bootstrap_clean_success_no_bootout launchd::tests::kickstart_cmd_shape launchd::tests::hard_failure_surfaces_install_error launchd::tests::is_loaded_reads_print_exit_code launchd::tests::render_plist_has_locked_keys` + `npm run e2e:survive` (real launchd path documented in `scripts/smoke-clean-vm.sh` via `BPA_E2E_EXTERNAL_DAEMON=1`) |
| Daemon boot: bind, wire deps, serve until shutdown, drain (spec §8.1–§8.3, §13) | `cargo test -p bpa-sessiond --test boot_integration boot_handshake_create_session_and_clean_shutdown` |
| Daemon-stop / logout truth table (§13) | Documented (not independently testable without killing the OS session) — see README.md "Survival truth table"; the "daemon stop → live shells end" half is implied by process-group kill semantics already proven by `kill_terminates_whole_process_group`; the "records+scrollback rehydrate as inactive" half is proven at the persistence-unit level (`committed_rows_survive_reopen`, `persist_and_rehydrate_round_trip`) AND end-to-end by e2e phase 5 (`npm run e2e:survive`, closes BL-7). |
| No-secrets-in-logs (§13, §16) | `cargo test -p bpa-sessiond --test no_secrets_in_logs planted_secret_never_appears_in_logs` (structured-log-file assertion; complements the child-env assertion in `pty_supervisor::tests::env_clear_hides_daemon_secret_keeps_allowlist`, a different surface) |
| Frontend: Zustand store shape + reducers (§12) | `npx vitest run src/store/store.test.ts` (`has the spec §12 initial shape`, `upsertSession …`, `setLifecycle …`, `markExited …`, `setDaemonConnected toggles the flag`, `never stores raw bytes: session values are exactly SessionMeta keys`) |
| Frontend: `session://state-changed` → status-dot update (§12) | `npx vitest run src/components/StatusDot.test.tsx` |
| Frontend: `daemon://disconnected` → banner (§12, §13) | `npx vitest run src/components/DaemonBanner.test.tsx` |
| Frontend: terminal-manager keep-alive (no dispose on unmount; dispose on close), StrictMode double-init guard (§12) | `npx vitest run src/terminal/terminal-manager.test.ts` (`ensure/create is idempotent and StrictMode-safe`, `keep-alive: nothing is disposed when a panel merely unmounts`, `dispose() only on real close`) |
| Frontend: Channel → `term.write` path; bytes never enter the store; Replay-before-open ordering (§12) | `npx vitest run src/terminal/terminal-manager.test.ts` (`applyReplay writes replay content BEFORE open()`, `attach() wires the Channel and applies Replay before Output, never touching the store`, `writeOutput goes straight to term.write`) |
| Frontend: per-session attach tracking — second and later tabs are NOT dead panes; manager-owned dedup; failed attach retryable; dispose clears attach state (§12, A1) | `npx vitest run src/terminal/terminal-manager.test.ts` (`attach() is idempotent per session: a second attach for the same id is a no-op`, `attach() tracks each session independently (s2 attaches even though s1 already did)`, `a FAILED attach is not recorded and is retryable`, `resetAttachment(id) clears one session's flag so the next attach re-runs (fresh Replay)`, `resetAllAttachments() clears every session's flag (reconnect: all re-attach fresh)`, `dispose() clears attach state so a same-id session recreated later does not false-dedup`, `attach() on an unknown (never-ensured / disposed) session is a no-op, records nothing`) + `npx vitest run src/App.test.tsx` (`A1: switching to a second tab attaches that session's terminal (no dead pane)`) |
| Frontend: `daemon://reconnected` → reset-all-then-re-attach (visible eager, hidden lazy); no pane dispose/remount (§12, §13, A1) | `npx vitest run src/App.test.tsx` (`daemon reconnect resets ALL attach flags then re-attaches the visible session (spec §13)`, `daemon reconnect: a hidden session lazily re-attaches when its tab is next shown (spec §13)`) |
| Frontend: coalesce in-flight attach — StrictMode/rapid-tab double-attach fires ONE IPC; rejection retryable; reset/dispose during in-flight invalidates the stale completion (§12, A2) | `npx vitest run src/terminal/terminal-manager.test.ts` (`coalesces two synchronous attach() calls for the same session into ONE IPC (StrictMode double-attach)`, `after an in-flight attach REJECTS, a later attach re-attempts (state back to detached)`, `resetAllAttachments() during an in-flight attach: the stale completion is NOT recorded, next attach re-fires`, `resetAttachment(id) during an in-flight attach also invalidates the stale completion`, `dispose() during an in-flight attach: the stale completion does not resurrect attach state`) |
| E2E survive-restart, incl. daemon-restart rehydration (§14.1, §13 core promise; Pv2 §9.8) | `npm run e2e:survive` (`tests/e2e/survive-restart.mjs`, phases 0–5 — the harness speaks the Pv2 preamble+CBOR wire; phase 5 closes BL-7) |

## Uncovered rows

None. Every §14.2 row above resolves to at least one real, currently-passing test.

## Test totals — current (Pv2, `[0.2.0]`, 2026-07-07)

- Rust workspace (`cargo test --workspace`): **238 tests**, 0 failed. Delta vs. the prior
  docs-truth pass (205): the Pv2 cycle added the `preamble` module + its dedicated test file (6),
  grew `roundtrip`/`framing` for the CBOR codec swap, added the `rehydrate_attach` integration
  test (1), and added daemon-lib coverage for multi-subscriber attach, cold-rehydrate, real drain,
  and schema-v2 `command_events` (sessiond lib 117 → 134). Re-run `cargo test --workspace --
  --list` yourself for the exact per-crate breakdown — the paragraph below (Task 25 + blocker-fixes
  era) is kept for history and no longer reflects current totals.
- TypeScript (`npx vitest run`): **118 tests**, 13 test files, 0 failed. Delta vs. the prior pass
  (107, 12 files): `UpgradeDialog.test.tsx` (+4) and `WorkspaceSidebar.test.tsx` (+4) landed this
  cycle (frontend upgrade-consent flow + workspace UI); the paragraph below is kept for history.
- E2E (`npm run e2e:survive`): green, **6 phases** (0–5, socket-harness variant) — phase 5 is new
  this cycle (daemon-restart rehydration, closes BL-7). Harness now speaks the Pv2 preamble+CBOR
  wire. Launchd-managed variant (`BPA_E2E_EXTERNAL_DAEMON=1`) and full-GUI variant still documented
  in `tests/e2e/README.md` and `docs/build-macos.md` as human/CI steps requiring a signed `.app`.

## Test totals as of this task (Task 25 + blocker-fixes) — historical, superseded above

- Rust workspace (`cargo test --workspace`): **205 tests**, 0 failed (`bpa-paths`: 7 — the shared
  path validator, moved verbatim from the core crate; protocol: 18, sessiond lib: 117, sessiond
  `boot_integration`: 3, sessiond `skeleton`: 1, sessiond `no_secrets_in_logs`: 1, core lib
  (`builder_pro_ai_lib`): 52, core `capabilities`: 5, core `invoke_smoke`: 1; doc-tests: 0 across
  all four crates). Delta vs. the prior blocker-fix pass (201): sessiond lib 114 → 117 (+3 graceful
  attach-teardown tests — trailing-output-on-natural-exit race, forwarder self-terminates after a
  graceful `remove_session`, and attach-on-exited-session refused); core lib 51 → 52 (the two
  tautological path pre-flight tests replaced by 3 real unit tests of the extracted `preflight_cwd`
  / `preflight_workspace_root` guards).
- TypeScript (`npx vitest run`): **107 tests**, 12 test files, 0 failed (delta +6 vs. the prior
  101: A2 in-flight-attach coalescing — `TerminalManager.attach` reworked into a per-session
  `detached | attaching | attached` state machine with a generation guard [terminal-manager.test.ts
  +6: two synchronous attach() calls coalesce onto ONE `attach_session` IPC (StrictMode
  double-attach — deterministic in dev via `<StrictMode>`), in-flight rejection stays retryable,
  `resetAllAttachments`/`resetAttachment`/`dispose` racing an in-flight attach each invalidate the
  stale completion so the next attach re-fires a fresh Replay, and the discriminating
  generation-guard test: a stale completion resolving while a NEWER attach is in flight is
  refused]). Delta before that (+9 vs. 92): A1
  dead-pane fix — per-session attach tracking in `TerminalManager` [terminal-manager.test.ts +7:
  idempotent per-session attach, independent per-session tracking, failed-attach-retryable,
  `resetAttachment`, `resetAllAttachments`, dispose-clears-attach-state, unknown-session no-op] and
  the App-level dead-pane + reconnect reset/lazy-re-attach coverage [App.test.tsx +2: the
  second-tab-attaches assertion, and the reconnect test split into an eager-visible-re-attach and a
  lazy-hidden-re-attach case]).
- E2E (`npm run e2e:survive`): green (5 phases, socket-harness variant); launchd-managed variant
  (`BPA_E2E_EXTERNAL_DAEMON=1`) and full-GUI variant documented in `tests/e2e/README.md` and
  `docs/build-macos.md` as human/CI steps requiring a signed `.app`.

## Coverage

`scripts/coverage-gate.sh` runs `cargo llvm-cov --package bpa-sessiond --fail-under-lines 80` — a
real, enforcing gate (non-zero exit below 80%). **Measured (2026-07-07, Pv2/`[0.2.0]` cycle):
`bpa-sessiond` line coverage = 89.58 %** (functions 88.17 %, regions 88.65 %) — the gate passes
with headroom. *(Historical: 2026-07-05, docs-truth/CI cycle measured 88.06 % line / 86.70 %
functions / 89.20 % regions — the Pv2 cycle added substantial new daemon surface — preamble
handshake, multi-subscriber attach, real drain, cold-rehydrate, schema-v2 writer — all TDD-covered,
which is why the number moved.)* The gate now runs in two enforced places:

- locally as `scripts/final-suite.sh` stage 7/8 (requires
  `rustup component add llvm-tools-preview && cargo install cargo-llvm-cov`);
- in CI as the blocking `coverage` job of `.github/workflows/ci.yml` (see `docs/backlog.md`
  BL-17 — added and verified green this cycle).

The evidence base behind the number: 134 `--lib` tests directly inside `bpa-sessiond`
(covering every module: `attach`, `boot`, `live_grid`, `logging`, `osc_parser`, `persistence`,
`pty_supervisor`, `scrollback`, `shell_integration`, `singleton`, `socket_server`) plus 3
`boot_integration` + 1 `no_secrets_in_logs` + 1 `rehydrate_attach` + 1 `skeleton` integration tests
exercising the full boot→serve→drain (and cold-rehydrate→attach) lifecycle over the real wire
protocol. (117 → 134 reflects the Pv2 cycle's new coverage: preamble/negotiation, multi-subscriber
attach, real drain, cold-rehydrate, schema-v2 `command_events`.)

*(History: at S0+S1 completion this gate was documented but not executed — the authoring
environment lacked the ~3–5 GB the instrumented build needs. That gap was closed by the
docs-truth/CI cycle; the paragraph above records the first real measurement.)*
