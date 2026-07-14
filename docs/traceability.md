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

## S2 contract rows (`docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md`)

Multi-root workspaces, a core-owned file explorer + read-only preview + live watch, an
attention-first Home, an OSC-133 command strip, and terminal file links — shipped `[0.3.0]`.

| Contract (S2 spec §) | Test (command) |
|---|---|
| Shared path-escape guard: `validate_path_within`/`validate_parent_within` — root itself, canonical-candidate-inside-root, `..`/symlink escape, non-existent create-parent, multi-segment/`.`/`..` final-component rejection (§4.1) | `cargo test -p bpa-paths` (`candidate_inside_root_returns_canonical_path`, `candidate_equal_to_root_is_ok`, `dotdot_escape_is_rejected`, `symlink_pointing_outside_root_is_rejected`, `symlink_within_parent_is_allowed`, `candidate_that_does_not_exist_fails_closed`, `parent_within_root_with_fresh_filename_is_ok`, `parent_within_nested_existing_dir_with_fresh_filename_is_ok`, `parent_within_multi_segment_final_component_is_rejected`, `parent_within_dotdot_final_component_is_rejected`, `parent_within_dot_final_component_is_rejected`, `parent_within_root_but_symlink_escaping_parent_is_rejected`) |
| Schema v3 `workspace_root` table + fail-closed forward-only v2→v3 migration; `root_path` stays the `ord=0` mirror (§3.2) | `cargo test -p bpa-sessiond --lib persistence::tests::fresh_db_is_v3_with_workspace_root_table persistence::tests::v2_db_migrates_to_v3_backfills_ord0_for_every_workspace persistence::tests::migration_v2_to_v3_fails_closed_on_error_and_leaves_v2_intact persistence::tests::v3_reopen_is_noop` |
| Multi-root persistence: ordered upsert/list, replace-not-accumulate on update, empty-roots rejected (§3.1-3.2) | `cargo test -p bpa-sessiond --lib persistence::tests::upsert_workspace_multi_root_then_list_preserves_order_and_root_path_mirror persistence::tests::upsert_workspace_replaces_roots_on_update_rather_than_accumulating persistence::tests::upsert_workspace_rejects_empty_roots` |
| `Add`/`RemoveWorkspaceRoot`: validate→append/renormalize ordering, reject removing the LAST root, idempotent no-op on an already-absent path, unknown-workspace error (§3.3) | `cargo test -p bpa-sessiond --lib persistence::tests::add_and_remove_workspace_root_ordering_and_renormalization persistence::tests::remove_last_workspace_root_is_rejected persistence::tests::remove_workspace_root_nonexistent_path_is_an_idempotent_noop persistence::tests::add_workspace_root_on_unknown_workspace_errors` |
| `Add`/`RemoveWorkspaceRoot` over the wire: persists + broadcasts `Push::WorkspaceUpdated` to OTHER clients too (Pv2 multi-subscriber), missing-dir rejected before persisting, last-root rejected with the `LastRoot` code (§3.3) | `cargo test -p bpa-sessiond --lib socket_server::tests::add_workspace_root_persists_and_broadcasts_to_other_clients socket_server::tests::add_workspace_root_rejects_missing_dir_and_persists_nothing socket_server::tests::remove_workspace_root_last_one_is_rejected_with_last_root_code socket_server::tests::remove_workspace_root_non_last_persists_and_broadcasts` |
| `GetCommandEvents`: newest-first, `limit` respected, unknown-session → empty (not an error) (§3.3) | `cargo test -p bpa-sessiond --lib socket_server::tests::get_command_events_returns_newest_first_and_respects_limit persistence::tests::list_command_events_respects_limit_and_unknown_session_is_empty` |
| `Workspace.roots` / `CommandEvent` Rust⇄TS parity (camelCase, `roots` array, `seq`/`ts` as `number`) (§3.1, §3.3) | `cargo test -p bpa-protocol --test ts_export` (`workspace_exposes_roots_array`, `command_event_is_exported_camelcase_with_number_seq_and_ts`) then `git diff --exit-code -- src/ipc/types.ts` |
| `Workspace`/`CommandEvent` CBOR round-trip, multi-root shape (§3.1, §3.3) | `cargo test -p bpa-protocol --test roundtrip` (`workspace_with_multiple_roots_roundtrips_via_cbor`, `command_event_roundtrips_via_cbor`) |
| `fs_explorer`: `listDir` one-level-lazy, `.gitignore`/nested-`.gitignore`/`.git`-always-hidden, outside-root rejected (§4.2) | `cargo test -p builder-pro-ai --lib fs_explorer::tests::list_dir_one_level_returns_files_and_dirs fs_explorer::tests::list_dir_second_level_is_lazy_via_rel fs_explorer::tests::list_dir_never_lists_dot_git fs_explorer::tests::list_dir_gitignored_entry_omitted_unless_include_ignored fs_explorer::tests::list_dir_respects_nested_gitignore fs_explorer::tests::list_dir_rejects_outside_root fs_explorer::tests::list_dir_on_a_file_is_not_found` |
| `fs_explorer`: `readFilePreview` — 1 MiB cap, text/binary/truncated detection (NUL/invalid-UTF8 in first 8 KiB probe), outside-root/directory rejected (§4.2) | `cargo test -p builder-pro-ai --lib fs_explorer::tests::build_preview_text_is_not_truncated_when_bytes_match_size fs_explorer::tests::build_preview_truncated_when_stat_size_exceeds_bytes_read fs_explorer::tests::build_preview_binary_on_nul_byte_in_first_probe_window fs_explorer::tests::build_preview_binary_on_invalid_utf8_in_first_probe_window fs_explorer::tests::build_preview_nul_beyond_probe_window_is_still_text fs_explorer::tests::read_file_preview_text_happy_path fs_explorer::tests::read_file_preview_binary_on_real_file fs_explorer::tests::read_file_preview_too_large_over_cap_never_reads_content fs_explorer::tests::read_file_preview_exactly_at_cap_is_text fs_explorer::tests::read_file_preview_rejects_outside_root fs_explorer::tests::read_file_preview_on_a_directory_is_an_honest_io_error` |
| `fs_explorer`: create/rename/move/delete(→Trash) — happy paths, outside-root on every side (src/dest), separator-in-name rejected, reveal/open outside-root rejected, every `FsError` variant camelCase-serializes (§4.2, §8) | `cargo test -p builder-pro-ai --lib fs_explorer::tests::create_file_happy_path fs_explorer::tests::create_file_does_not_overwrite_existing fs_explorer::tests::create_file_rejects_outside_root fs_explorer::tests::create_file_rejects_separator_in_name fs_explorer::tests::create_dir_happy_path fs_explorer::tests::create_dir_rejects_outside_root fs_explorer::tests::rename_entry_happy_path fs_explorer::tests::rename_entry_nested_happy_path fs_explorer::tests::rename_entry_rejects_outside_root_source fs_explorer::tests::rename_entry_rejects_separator_in_new_name fs_explorer::tests::move_entry_happy_path fs_explorer::tests::move_entry_rejects_outside_root_destination fs_explorer::tests::move_entry_rejects_outside_root_source fs_explorer::tests::delete_entry_rejects_outside_root fs_explorer::tests::delete_entry_moves_file_out_of_its_original_location fs_explorer::tests::reveal_in_finder_rejects_outside_root fs_explorer::tests::open_external_rejects_outside_root fs_explorer::tests::fs_error_serializes_with_camel_case_tag fs_explorer::tests::file_preview_serializes_with_camel_case_tag fs_explorer::tests::fs_entry_serializes_camel_case_fields` |
| `fs_watcher`: debounced FSEvents watch, gitignore filter (root-level + `.git`-always), dedupe, 500-path cap → `["*"]` overflow sentinel, multi-root routing, `fs://watch-error` on nonexistent/failed root, stop/restart lifecycle, camelCase payload shape, real `notify` integration (§5) | `cargo test -p builder-pro-ai --lib fs_watcher::tests::build_root_gitignore_matches_root_patterns fs_watcher::tests::gitignored_path_dropped_unless_show_ignored fs_watcher::tests::dot_git_internal_paths_always_dropped_even_with_show_ignored fs_watcher::tests::path_outside_all_roots_is_dropped fs_watcher::tests::duplicate_paths_are_deduped fs_watcher::tests::over_cap_paths_collapse_to_refresh_everything_sentinel fs_watcher::tests::exactly_at_cap_is_not_collapsed fs_watcher::tests::multiple_roots_routed_to_the_right_root fs_watcher::tests::watch_error_with_matching_path_routes_to_its_root fs_watcher::tests::watch_error_without_path_info_surfaces_against_every_root fs_watcher::tests::nonexistent_root_emits_watch_error_and_is_excluded_from_matchers fs_watcher::tests::changed_payload_uses_camel_case_changed_rel_paths_key fs_watcher::tests::watch_error_payload_shape fs_watcher::tests::stop_on_an_already_empty_slot_is_a_harmless_noop fs_watcher::tests::starting_again_replaces_the_previous_watch_state fs_watcher::tests::real_notify_watch_delivers_debounced_changed_event_filters_gitignore_and_respects_stop fs_watcher::tests::watch_start_failure_on_a_nonexistent_root_emits_watch_error_not_a_panic` |
| Frontend `fs`-slice + IPC wrappers: `treeCache`/`expanded`/`selectedFile` reducers, point-refresh `invalidateDirs` (keeps `expanded`, `["*"]` clears a whole root), typed `fs.ts` wrappers, `onFsChanged`/`onFsWatchError`/`onWorkspaceUpdated` listeners, `addWorkspaceRoot`/`removeWorkspaceRoot`/`getCommandEvents` command wrappers (§6.6) | `npx vitest run src/store/store.test.ts src/ipc/fs.test.ts src/ipc/events.test.ts src/ipc/commands.test.ts` |
| `FileTree`: lazy per-level fetch + cache, windowed rendering (<500 DOM rows @10k), dirs-first sort, ignored-dimmed behind toggle, context menu (new file/folder/rename/delete→Trash-with-confirm/reveal/open-external), root «+ Add root», FsError→toast on every op (§6.4) | `npx vitest run src/components/FileTree.test.tsx` (20 tests) |
| `FilePreview`: text/binary/tooLarge/error placeholders, re-fetch on selection change, stale-request guard, error also fires a toast (§6.4, §7) | `npx vitest run src/components/FilePreview.test.tsx` (10 tests) |
| `FilesRail`: collapsed/expanded, show-ignored toggle invalidates+refetches, watch-paused affordance restarts the watch (§6.4, §5) | `npx vitest run src/components/FilesRail.test.tsx` (8 tests) |
| `HomeView` attention-first ordering: waiting pinned first (exited always wins over a stale flag), «Пройти →» / row-click navigate+activate+focus, stats strip, ✓/✗ exited rows, empty states (§6.2) | `npx vitest run src/components/HomeView.test.tsx` (12 tests) + `npx vitest run src/App.test.tsx` (`end-to-end: Пройти from Home switches to the workspace view with that session active and focuses its terminal`) |
| Workspace stat chips (live/waiting/exited/roots, scoped to the active workspace, click-to-expand detail) + root-aware new-terminal cwd (§6.3) | `npx vitest run src/App.test.tsx` (`stat chips show correct live/waiting/exited/roots counts, scoped to the active workspace only`, `clicking a stat chip toggles an inline detail list; clicking it again closes it`, `stat chips render nothing while no workspace is active`) |
| `CommandStrip`: ✓/✗ chips from `command_events`, running-dot for an unmatched `started`, newest-first pairing, empty is calm (not an error), fetch failure toasts + renders nothing, refetches on lifecycle/exit change scoped to its own session (§6.3) | `npx vitest run src/components/CommandStrip.test.tsx` (10 tests) |
| Terminal file links: lexical token patterns (absolute/dot-relative/extensioned-relative), `:line[:col]` suffix stripped from the path but kept in the span, `~/` detected-then-skipped, prose/bare-word rejected, `endCol` xterm-inclusive conversion (§6.5, D9) | `npx vitest run src/terminal/link-provider.test.ts` (18 tests) |

## S3 contract rows (`docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md`)

The second launchd daemon `bpa-orchd` + the app-domain foundation (six entity families, RuleSet
markdown files, export/import, project management UI) — shipped `[0.4.0]`.

| Contract (S3 spec §) | Test (command) |
|---|---|
| `bpa-daemon-core` extraction: shared migration runner (whole-chain, skip-below-`from`, mid-chain rollback, `VersionTooNew`) (§3, D2) | `cargo test -p bpa-daemon-core --lib migrate::tests::whole_chain_success_applies_every_step_and_reaches_target migrate::tests::steps_at_or_below_from_version_are_skipped migrate::tests::mid_chain_failure_rolls_back_the_whole_chain_not_just_the_failing_step migrate::tests::version_too_new_is_rejected_without_touching_the_db migrate::tests::empty_steps_with_from_equal_target_zero_is_ok migrate::tests::version_too_new_message_matches_expected_wire_text` |
| `bpa-daemon-core` extraction: shared codec-agnostic preamble accept/negotiate (accept/incompatible/garbage/timeout) (§3, D2) | `cargo test -p bpa-daemon-core --lib handshake::tests::compatible_versions_accept_and_echo_the_passed_build handshake::tests::disjoint_ranges_are_incompatible_and_daemon_range_is_reported handshake::tests::garbage_magic_returns_err handshake::tests::stalled_client_times_out_with_err_within_preamble_timeout` |
| `bpa-daemon-core` extraction: generic `Broadcaster<F>` client-push registry (both daemons instantiate the SAME generic type) (§3, D2) | `cargo test -p bpa-daemon-core --lib broadcast::tests::two_registered_receivers_both_get_the_broadcast_value broadcast::tests::full_receiver_queue_is_skipped_without_blocking_other_receivers broadcast::tests::deregistered_receiver_gets_nothing broadcast::tests::clone_shares_the_same_registry` |
| `bpa-daemon-core` extraction: shared singleton (flock, socket path `$XDG_RUNTIME_DIR/bpa` else `/tmp/bpa-{uid}`, dir/socket perms, peer-cred) (§3, D2) | `cargo test -p bpa-daemon-core --lib singleton::tests::resolve_socket_path_and_lockfile_end_with_given_names_under_xdg_runtime_dir singleton::tests::resolve_socket_path_falls_back_to_tmp_with_uid_when_xdg_unset singleton::tests::socket_path_len_under_104_passes_and_over_fails singleton::tests::ensure_dir_creates_with_0700 singleton::tests::ensure_dir_refuses_world_writable_squat singleton::tests::second_acquire_lock_at_on_same_path_would_block singleton::tests::lockfile_created_mode_0600 singleton::tests::peer_cred_accepts_same_uid_over_socketpair singleton::tests::peer_cred_rejects_foreign_uid_simulated` |
| `bpa-sessiond` re-seated on daemon-core: on-disk socket/lock/plist paths byte-identical, whole sessiond regression net still green (Phase-1 regression net, §12) | `cargo test -p bpa-sessiond --lib` (155 tests, all pass post-extraction) + `cargo test -p builder-pro-ai --lib launchd::tests::` (sessiond plist bytes asserted identical pre/post extraction) |
| `bpa-orchd-proto` CBOR round-trip, every `OrchdFrame`/`OrchdRequest`/`OrchdResponse`/`OrchdPush`/entity-struct variant; version consts locked at `1` (§4.2, D8) | `cargo test -p bpa-orchd-proto --test roundtrip` (8 tests) |
| `bpa-orchd-proto` ⇄ TS parity (`orchd-types.ts` generated, camelCase, entity shapes) (§4.2) | `cargo test -p bpa-orchd-proto --test ts_export` (11 tests) then `git diff --exit-code -- src/ipc/orchd-types.ts` (wired into `scripts/final-suite.sh` stage 6/9) |
| orchd schema v1: every table created, FK enforcement, corrupt-DB quarantine + recreate (`orchd.db.corrupt-<ts>`), WAL checkpoint no-op-safe (§5.1) | `cargo test -p bpa-orchd --lib persistence::tests::open_in_memory_creates_schema_v1_with_every_table persistence::tests::open_on_disk_creates_schema_v1_with_every_table persistence::tests::foreign_keys_are_enforced persistence::tests::corrupt_db_is_quarantined_and_recreated persistence::tests::checkpoint_on_disk_db_does_not_error persistence::tests::checkpoint_on_in_memory_db_does_not_error` |
| Project CRUD + invariants: `CreateProject` auto-creates strategic goal + ruleset row, empty-workspace-ids/cross-project-link/duplicate-in-call invariants, archive blocks every mutating child verb (§5.2) | `cargo test -p bpa-orchd --lib persistence::tests::create_project_creates_strategic_goal_and_ruleset_row persistence::tests::create_project_empty_workspace_ids_is_invariant persistence::tests::create_project_workspace_linked_to_another_project_is_conflict persistence::tests::archive_project_sets_status_archived persistence::tests::archived_project_blocks_update_project persistence::tests::archived_project_blocks_create_goal persistence::tests::archived_project_blocks_create_idea persistence::tests::archived_project_blocks_create_insight persistence::tests::archived_project_list_goals_still_works persistence::tests::add_project_workspace_appends_and_conflicts_when_linked_elsewhere persistence::tests::remove_project_workspace_last_link_is_invariant` |
| Goal tree CRUD + invariants: exactly one `strategic` root, arbitrary-depth `additional` via `parent_id`, move/cycle-guard, delete-subtree cascade, parents-before-children ordering (§5.2, D5) | `cargo test -p bpa-orchd --lib persistence::tests::create_goal_second_strategic_is_invariant persistence::tests::create_goal_additional_without_parent_is_invariant persistence::tests::create_goal_cross_project_parent_is_invariant persistence::tests::create_goal_ord_increments_per_sibling_group persistence::tests::move_goal_strategic_root_is_invariant persistence::tests::move_goal_under_own_descendant_or_self_is_cycle_invariant persistence::tests::move_goal_updates_parent_and_ord persistence::tests::delete_goal_strategic_is_invariant persistence::tests::delete_goal_cascades_subtree persistence::tests::list_goals_parents_before_children_then_ord` |
| Idea CRUD + lifecycle + `SetIdeaProject` (nullable `project_id`, orphan ideas mutable, attach/detach) (§5.2, D3/D11) | `cargo test -p bpa-orchd --lib persistence::tests::create_idea_defaults_lifecycle_captured_orphan_by_default persistence::tests::list_ideas_none_includes_orphans_but_project_filter_excludes_them persistence::tests::orphan_idea_remains_mutable_with_no_project persistence::tests::set_idea_project_none_detaches persistence::tests::set_idea_project_attaches_orphan_to_a_project persistence::tests::set_idea_project_blocked_when_target_project_archived persistence::tests::set_idea_lifecycle_persists_snake_case_db_literal persistence::tests::delete_idea_removes_row` |
| Insight CRUD + fit-verdict override + archive-requires-reasoning (§5.2) | `cargo test -p bpa-orchd --lib persistence::tests::create_insight_defaults_status_new_orphan_by_default persistence::tests::set_insight_fit_verdict_stores_verdict_and_reasoning persistence::tests::set_insight_fit_verdict_none_clears_verdict_but_keeps_reasoning persistence::tests::set_insight_status_updates_status_and_resolution_reasoning persistence::tests::set_insight_status_none_reasoning_leaves_it_unchanged persistence::tests::orphan_insight_remains_mutable_with_no_project` |
| Task/Subtask CRUD + `rank` midpoint reordering (§5.2) | `cargo test -p bpa-orchd --lib persistence::tests::create_task_rank_sequence_1024_2048_3072` (+ the full `persistence::tests::` task/rank suite — `cargo test -p bpa-orchd --lib persistence::tests::` lists every `*_task_*`/`*_rank_*` name) |
| RuleSet DB row CRUD + `PolicyRules` strict-validation (unknown key rejected, round-trips through the wire type) (§5.2, §7) | `cargo test -p bpa-orchd --lib persistence::tests::` (`validate_policy_rejects_an_unknown_json_key` and the `ruleset`/`policy`-prefixed tests in that module) |
| RuleSet files: atomic write (tmp+rename, no leftover tmp, parent dirs created), fresh-read-on-Get state machine (`Ok`/`ExternallyModified`/`Missing`) (§7, D4) | `cargo test -p bpa-orchd --lib ruleset_files::tests::write_atomic_creates_parent_dirs_and_returns_the_exact_content_hash ruleset_files::tests::write_atomic_leaves_no_tmp_file_behind ruleset_files::tests::write_atomic_overwrites_an_existing_file ruleset_files::tests::read_state_missing_file_is_missing_with_no_content ruleset_files::tests::read_state_matching_hash_is_ok_with_content ruleset_files::tests::read_state_mismatched_hash_is_externally_modified_with_the_new_content ruleset_files::tests::read_state_unreadable_path_is_missing` |
| Export/import: round-trip identical modulo `exportedAt`, field-verbatim preservation (`updated_at`/`rank` never re-stamped), id collision → `Conflict` + full rollback, 16 MiB frame-cap guard, ruleset md path containment (rejects `..`/path-separator, repoints outside-app-support imports) (§8, D7) | `cargo test -p bpa-orchd --lib export::tests::export_import_round_trip_is_semantically_identical_modulo_exported_at export::tests::import_preserves_updated_at_and_rank_verbatim_not_freshly_stamped export::tests::import_task_id_collision_is_conflict_and_rolls_back_everything export::tests::export_project_over_16_mib_is_an_io_frame_cap_error export::tests::export_all_over_16_mib_is_an_io_frame_cap_error export::tests::export_project_ruleset_with_a_missing_file_has_null_md_content export::tests::export_project_ruleset_with_an_empty_file_has_empty_string_md_content_not_null export::tests::import_repoints_a_foreign_ruleset_md_path_under_the_given_app_support export::tests::import_rejects_a_ruleset_md_path_with_dotdot_traversal_and_writes_nothing_outside export::tests::import_rejects_a_ruleset_project_id_with_a_path_separator export::tests::import_into_a_boot_seeded_store_reconciles_the_global_ruleset` |
| Socket dispatch, end-to-end over a real Unix socket: mutate → response + a SECOND connection receives the matching `orchd://*-changed` push; failed mutate broadcasts nothing (§4.2, §5, §6, §7) | `cargo test -p bpa-orchd --test dispatch_integration` (7 tests: `create_project_returns_project_broadcasts_projects_changed_and_writes_ruleset_file`, `create_goal_broadcasts_goals_changed_with_correct_project_id`, `remove_last_project_workspace_is_invariant_and_broadcasts_nothing`, `get_ruleset_ok_then_externally_modified_after_on_disk_edit`, `set_idea_project_none_detaches`, `import_bundle_happy_path_returns_report_and_broadcasts_family_pushes`, `unknown_id_delete_task_is_not_found`) |
| Daemon boot: bind → wire deps → serve → drain; global ruleset row + `rules/global.md` ensured idempotently at every boot; second-instance flock refusal (§5, §9) | `cargo test -p bpa-orchd --test boot_integration` (4 tests: `boot_handshake_ping_pong_and_clean_shutdown`, `second_instance_flock_refusal`, `fresh_boot_creates_schema_v1_and_global_ruleset`, `double_boot_does_not_duplicate_global_ruleset_row`) |
| No-secrets-in-logs, orchd (§13, §16) | `cargo test -p bpa-orchd --test no_secrets_in_logs planted_ruleset_secrets_never_appear_in_logs` |
| Core: `OrchdClient`/error mapping, `orchd_*` commands over a stub daemon (real socket, `ENV_TEST_LOCK` discipline, version consts not literals), upgrade/reconnect choreography (§9) | `cargo test -p builder-pro-ai --lib commands::` (`orchd_client_error_disconnected_maps_to_command_error_disconnected`, `orchd_client_error_daemon_maps_to_command_error_daemon`, `orchd_client_error_incompatible_orchd_maps_to_command_error_incompatible_orchd`, `orchd_upgrade_core_drains_then_kickstarts`, `orchd_upgrade_core_without_client_still_kickstarts`, `orchd_create_project_round_trips_through_real_orchd_client`, `orchd_invariant_error_response_becomes_command_error_daemon_invariant_end_to_end`, `orchd_reveal_rules_file_core_uses_the_get_rule_set_returned_path`, `orchd_export_to_file_all_export_writes_json_named_store`, `orchd_export_to_file_project_export_uses_sanitized_project_name`, `orchd_import_from_file_reads_file_and_round_trips_through_stub`, `orchd_import_from_file_refuses_a_file_over_the_10_mib_cap`) |
| Core: `map_orchd_push` — one test per `OrchdPush` variant → the matching `orchd://*-changed` `Emit` action, correct camelCase payload; `map_orchd_conn_state` — every variant (§9, D6, D10) | `cargo test -p builder-pro-ai --lib broker::tests::orchd_projects_changed_maps_to_emit_with_null_payload broker::tests::orchd_goals_changed_maps_to_emit_with_camel_case_project_id_payload broker::tests::orchd_ideas_changed_maps_to_emit_with_null_payload broker::tests::orchd_insights_changed_maps_to_emit_with_null_payload broker::tests::orchd_tasks_changed_maps_to_emit_with_camel_case_project_id_payload broker::tests::orchd_ruleset_changed_maps_to_emit_with_scope_and_project_id_payload broker::tests::orchd_ruleset_changed_global_scope_has_null_project_id broker::tests::map_orchd_conn_state_maps_every_variant` |
| `bring_up_orchd`: bounded-retry connect, `IncompatibleOrchd` never retried, status transitions (§9) | `cargo test -p builder-pro-ai --lib lib::tests::connect_orchd_with_retry_gives_up_after_bounded_attempts_without_panicking lib::tests::connect_orchd_with_retry_does_not_retry_incompatible lib::tests::status_for_orchd_connect_result_maps_incompatible_orchd lib::tests::status_for_orchd_connect_result_maps_ok_to_connected lib::tests::status_for_orchd_conn_state_maps_every_variant lib::tests::orchd_event_names_are_locked` |
| `launchd.rs`: orchd identity (`ORCHD_LABEL`, `orchd.out.log`/`orchd.err.log`) rendered into its OWN plist/service-target, sessiond's stays byte-identical (§9) | `cargo test -p builder-pro-ai --lib launchd::tests::render_plist_and_service_target_use_orchd_identity_for_orchd_agent` (+ the pre-existing sessiond-identity plist tests, unchanged, proving no cross-contamination) |
| Frontend `domainSlice` + IPC wrappers (`orchd.ts`, `onOrchd*` events) (§10) | `npx vitest run src/store/store.test.ts src/ipc/orchd.test.ts src/ipc/events.test.ts` (52 + 52 + 20 tests) |
| `ProjectPanel` (tabs) / `GoalTree` / `IdeasList` / `TasksList` / `InsightsList` / `RulesetPanel` / `CreateProjectDialog` / left-rail project groups (§10) | `npx vitest run src/components/ProjectPanel.test.tsx src/components/GoalTree.test.tsx src/components/IdeasList.test.tsx src/components/TasksList.test.tsx src/components/InsightsList.test.tsx src/components/RulesetPanel.test.tsx src/components/CreateProjectDialog.test.tsx src/components/WorkspaceSidebar.test.tsx` (10 + 9 + 10 + 12 + 7 + 12 + 11 + 11 tests) |
| `QuickCapture` (⌘K overlay, disabled honestly while `orchdDown`), `HomeGoals` (mounted below the amber attention block), `OrchdDownBanner` / dual-daemon `UpgradeDialog` generalization (§10, §11) | `npx vitest run src/components/QuickCapture.test.tsx src/components/HomeGoals.test.tsx src/components/OrchdDownBanner.test.tsx src/components/UpgradeDialog.test.tsx` (13 + 8 + 2 + 16 tests) |
| E2E orchd survive-restart + export/import round-trip: boot → handshake `[1,1]` → create project+goals+idea+task → `OrchdShutdown{drain:true}` → relaunch → data intact → `ExportAll` → wipe `orchd.db*` → relaunch fresh v1 → `ImportBundle` → re-export equals the original modulo `exportedAt` (§12 — the roadmap DoD proof) | `npm run e2e:orchd` (`tests/e2e/orchd-survive.mjs`, phases 0-4, log format `[e2e-orchd] phaseN OK: …` / `[e2e-orchd] ALL PHASES PASSED`) |

## S4 contract rows (`docs/superpowers/specs/2026-07-14-s4-knowledge-graph-design.md`)

The knowledge graph (`orchd.db` schema v2, `graph_node`/`graph_edge`), the workspace-wide agent
retrieval API, and the `@xyflow/react` graph canvas — shipped `[0.5.0]`.

| Contract (S4 spec §) | Test (command) |
|---|---|
| Schema v2: fresh DB has both tables + all 5 indexes; v1→v2 migration backfills a strategic-goal `entityRef` node per pre-S4 project (single- and multi-project fixtures) (§4) | `cargo test -p bpa-orchd --lib graph::tests::fresh_db_is_schema_v2_with_graph_tables_and_all_five_indexes graph::tests::v1_fixture_migrates_to_v2_and_backfills_strategic_entity_ref graph::tests::v1_fixture_with_multiple_projects_backfills_each_independently graph::tests::create_project_auto_seeds_strategic_entity_ref_node` |
| Node CRUD + invariants: `add_node` happy path, `EntityRef` kind rejected as `Validation` (entityRef only via the internal seeder), unknown/archived-project guards, `add_entity_ref_node` + duplicate-`(entity_type,entity_id)`→`Conflict`, update/move/delete + their archived guards, delete cascades incident edges (§5) | `cargo test -p bpa-orchd --lib graph::tests::add_node_happy_path_creates_concept_node graph::tests::add_node_rejects_entity_ref_kind_with_validation graph::tests::add_node_unknown_project_is_not_found graph::tests::add_node_on_archived_project_is_invariant graph::tests::add_entity_ref_node_happy_path graph::tests::add_entity_ref_node_duplicate_type_and_id_is_conflict graph::tests::add_entity_ref_node_on_archived_project_is_invariant graph::tests::update_node_updates_label_and_body_independently graph::tests::update_node_unknown_id_is_not_found graph::tests::update_node_on_archived_project_is_invariant graph::tests::move_node_updates_position graph::tests::move_node_on_archived_project_is_invariant graph::tests::delete_node_cascades_incident_edges graph::tests::delete_node_unknown_id_is_not_found graph::tests::delete_node_on_archived_project_is_invariant` |
| Edge CRUD + invariants: cross-project create, self-loop→`Invariant`, duplicate `(source,target,kind)`→`Conflict`, unknown endpoint→`NotFound`, archived guard on EITHER endpoint, delete + its archived guard (§5) | `cargo test -p bpa-orchd --lib graph::tests::add_edge_cross_project_ok graph::tests::add_edge_self_loop_is_invariant graph::tests::add_edge_duplicate_source_target_kind_is_conflict graph::tests::add_edge_unknown_endpoint_is_not_found graph::tests::add_edge_blocked_when_source_project_archived graph::tests::add_edge_blocked_when_target_project_archived graph::tests::delete_edge_removes_row graph::tests::delete_edge_unknown_id_is_not_found graph::tests::delete_edge_on_archived_project_endpoint_is_invariant` |
| `entityRef` soft-ref survival across a non-strategic domain-entity delete (D3 — the node persists, no FK); `edge_endpoint_projects`/`node_project_ids_reachable` helpers (used by dispatch to fan out `GraphChanged`) (§5, §6) | `cargo test -p bpa-orchd --lib graph::tests::entity_ref_node_survives_deletion_of_its_non_strategic_source_idea graph::tests::edge_endpoint_projects_returns_both_projects graph::tests::edge_endpoint_projects_unknown_edge_is_not_found graph::tests::node_project_ids_reachable_returns_own_and_foreign_projects graph::tests::node_project_ids_reachable_returns_only_own_when_no_edges graph::tests::node_project_ids_reachable_unknown_node_is_not_found` |
| `list_project_graph`: own nodes + incident edges + cross-project `external_nodes` ghosts (deduped across multiple edges to the same foreign node), read-time `entityRef` label resolution (a renamed source's live title, and the stored label kept + orphan-flagged when the source is deleted — in BOTH `nodes` and `external_nodes`) (§5) | `cargo test -p bpa-orchd --lib graph::tests::list_project_graph_unknown_project_is_not_found graph::tests::list_project_graph_includes_own_nodes_edges_and_cross_project_external_ghost graph::tests::list_project_graph_dedupes_external_ghost_reached_by_multiple_edges graph::tests::list_project_graph_resolves_entity_ref_label_from_renamed_source_at_read_time graph::tests::list_project_graph_keeps_stored_label_when_entity_ref_source_is_deleted graph::tests::list_project_graph_resolves_entity_ref_label_in_external_nodes_too` |
| `neighborhood`: unknown root→`NotFound`, exact N-hop reachable set across a cross-project edge, depth clamped at 6, both-direction traversal; **Perf DoD:** depth-3 neighborhood rooted at the D6-seeded strategic-goal node on a synthetic 500-node/1000-edge graph is `<100 ms` (measured ~51 ms locally) (§5, §8) | `cargo test -p bpa-orchd --lib graph::tests::neighborhood_unknown_node_id_is_not_found graph::tests::neighborhood_depth_2_returns_exact_2hop_reachable_set_across_cross_project_edge graph::tests::neighborhood_depth_over_6_is_clamped_to_6 graph::tests::neighborhood_traverses_edges_in_both_directions graph::tests::neighborhood_depth_3_on_500_node_1000_edge_graph_is_under_100ms_rooted_at_goal_node` |
| `search_nodes`: workspace-wide (`project_id: None`) vs. project-scoped, matches `label` and `body`, case-insensitive, `updated_at DESC` ordering, capped at 200 rows (§5) | `cargo test -p bpa-orchd --lib graph::tests::search_nodes_none_project_spans_workspace graph::tests::search_nodes_some_project_scopes_to_that_project graph::tests::search_nodes_matches_body_too graph::tests::search_nodes_is_case_insensitive graph::tests::search_nodes_orders_by_updated_at_desc graph::tests::search_nodes_caps_at_200_rows` |
| Socket dispatch over a real Unix socket: mutate → response + the correct `GraphChanged` push(es) — same-project edge dedups to exactly ONE push, a cross-project edge/node update/move/delete pushes BOTH affected projects; read verbs (`GraphListProject`/`GraphNeighborhood`/`GraphSearch`) broadcast nothing; a failed mutation (self-loop) broadcasts nothing (§6) | `cargo test -p bpa-orchd --test dispatch_integration graph_add_node_returns_node_and_broadcasts_graph_changed_to_its_project graph_add_edge_cross_project_broadcasts_graph_changed_for_both_endpoint_projects graph_add_edge_same_project_broadcasts_exactly_one_graph_changed graph_delete_node_cross_project_broadcasts_graph_changed_for_foreign_project_too graph_update_node_and_move_node_cross_project_broadcast_foreign_project_too graph_delete_edge_cross_project_broadcasts_graph_changed_for_both_endpoint_projects graph_list_project_returns_view_and_broadcasts_nothing graph_add_edge_self_loop_is_invariant_and_broadcasts_nothing graph_neighborhood_returns_correct_subgraph graph_search_returns_matching_nodes_workspace_wide_and_broadcasts_nothing` |
| `bpa-orchd-proto`: every graph `OrchdRequest`/`OrchdResponse`/`OrchdPush` variant CBOR round-trips (incl. every `GraphNodeKind`/`GraphEdgeKind`/`GraphEntityType` wire-tag literal); `orchd-types.ts` parity (camelCase fields, `i64` timestamps exported as `number`) (§3) | `cargo test -p bpa-orchd-proto --test roundtrip` (`every_request_variant_roundtrips`, `every_response_variant_roundtrips`, `every_push_variant_roundtrips`, `graph_node_kind_entity_ref_serializes_as_camelcase_on_the_wire`, `graph_edge_kind_contradicts_serializes_lowercase_on_the_wire`, `graph_entity_type_task_serializes_lowercase_on_the_wire`) + `cargo test -p bpa-orchd-proto --test ts_export` (`graph_node_and_edge_use_camelcase_fields_and_ts_number_timestamps`, `graph_node_kind_and_edge_kind_and_entity_type_wire_tags_are_camelcase`) then `git diff --exit-code -- src/ipc/orchd-types.ts` |
| Core: 9 `orchd_graph_*` commands over a stub daemon (real socket), invariant-error mapping; `map_orchd_push` for `OrchdPush::GraphChanged` → `orchd://graph-changed` with a camelCase `{projectId}` payload (§7) | `cargo test -p builder-pro-ai --lib commands::orchd_graph_add_node_round_trips_through_real_orchd_client commands::orchd_graph_add_node_invariant_error_response_becomes_command_error_daemon_invariant` + `cargo test -p builder-pro-ai --lib broker::tests::orchd_graph_changed_maps_to_emit_with_camel_case_project_id_payload` |
| `graphMapping.ts` (PURE, no xyflow/React import): `toFlowNodes`/`toFlowEdges` incl. ghost (`isExternal`)/orphan (`isOrphan`) flags + position mapping, `flowPositionChangeToMove` (position vs. non-position changes), `dedupeMovesById` debounce-collapse contract (§7, D10) | `npx vitest run src/components/graph/graphMapping.test.ts` (12 tests) |
| `GraphCanvas` (rendered under `// @vitest-environment jsdom` + the `mockReactFlow()` shim, D10): mount-refresh, `onConnect`→`orchdGraphAddEdge`, a position `onNodesChange` debounces (400 ms) + dedupes to ONE `orchdGraphMoveNode` call, toolbar add/delete(+confirm)/search (incl. a stale-response search-race guard and a partial-multi-delete reconcile-via-`refreshGraph`), ghost-node click→`openProject`, a LOCAL `entityRef` node click is a documented no-op, every mutating control `disabled` while `orchdDown`, a failed mutation surfaces via toast (§7, D10) | `npx vitest run src/components/graph/GraphCanvas.test.tsx` (19 tests) |
| `ProjectPanel`'s 7th tab «Граф» renders `GraphCanvas` (mocked, as every other tab child) and selects correctly (§7) | `npx vitest run src/components/ProjectPanel.test.tsx` |
| Store `graphByProject` slice: `refreshGraph(projectId)` replaces only that project's entry, a rejection surfaces via toast; `orchd.ts` graph wrapper name/arg parity; `onOrchdGraphChanged` binds unconditionally to `refreshGraph` (App mount effect, no loaded/active gating — matches the other `orchd://*-changed` bindings) (§7) | `npx vitest run src/store/store.test.ts src/ipc/orchd.test.ts src/ipc/events.test.ts` |
| E2E — cross-project edge survives BOTH projects' daemon restarts (the S4 spec §8 DoD proof): create 2 projects, 1 node each, a cross-project edge, `OrchdShutdown{drain:true}` → relaunch → `GraphListProject` on EITHER project still shows the edge + the foreign node as an `external_nodes` ghost (§8) | `npm run e2e:orchd` (`tests/e2e/orchd-survive.mjs`, phase 5, log prefix `[e2e-orchd] phase5 …`) |
| No-secrets-in-logs: a graph node's `label`/`body` never reach the tracing log sink (§8 — the extension the spec called for "extend orchd's `no_secrets_in_logs` test (or its graph-covering sibling)"). Plants two distinct secret markers (one per field) on a real on-disk `Db`, drives `create_project` → `add_node` → `update_node` → `add_edge` → a duplicate `add_edge` (`Conflict`) → a self-loop `add_edge` on the same node (`Invariant`) → `delete_node`, flushes the real sink, asserts neither marker appears in the log file. RED-proven (BL-62 closure): a temporary deliberate leak inserted into the test driver made the assertion fail before removal, confirming the test genuinely catches a leak (§8) | `cargo test -p bpa-orchd --test no_secrets_in_logs_graph planted_graph_node_secrets_never_appear_in_logs` |

## Uncovered rows

None in the S0+S1/S2/S3 rows above — every §14.2 row (and every S2/S3 contract row) resolves to at
least one real, currently-passing test. **None in the S4 section above either:** the graph
no-secrets-in-logs coverage (formerly BL-62, "Known gap") is now covered by the row directly above
— every S4 contract row resolves to at least one real, currently-passing test.

## Test totals — current (S4, `[0.5.0]`, 2026-07-14)

- Rust workspace (`cargo test --workspace`): **727 tests**, 0 failed (BL-62 follow-up: +1 vs. the
  726 recorded below at S4's `[0.5.0]` release — the new `bpa-orchd` `no_secrets_in_logs_graph`
  binary, `planted_graph_node_secrets_never_appear_in_logs`; every other per-crate count below is
  unchanged by that follow-up. Original per-binary breakdown from the `[0.5.0]` measurement pass:
  `bpa-daemon-core` lib 29 [unchanged — S4 does not touch
  daemon-core]; `bpa-orchd` lib 211 + `boot_integration` 4 + `dispatch_integration` 17 +
  `no_secrets_in_logs` 1 (+ `no_secrets_in_logs_graph` 1 as of the BL-62 follow-up); `bpa-orchd-proto` `roundtrip` 11 + `ts_export` 13; `bpa-paths` lib 18
  [unchanged]; `bpa-protocol` lib 1 + `cbor_frame_generic` 7 + `framing` 7 + `preamble` 7 +
  `roundtrip` 8 + `ts_export` 7 [unchanged — S4 touches no sessiond/protocol code]; `bpa-sessiond`
  lib 155 + `boot_integration` 4 + `no_secrets_in_logs` 1 + `rehydrate_attach` 1 + `skeleton` 1
  [unchanged — S4 spec §2 "NO sessiond change", confirmed by this identical count]; `builder-pro-ai`
  lib 217 + `capabilities` 5 + `invoke_smoke` 1; every `main.rs`/doc-test binary 0). Delta vs. the
  prior S3 pass (655): **+71**, entirely inside the orchd family — `bpa-orchd` lib 158→211 (+53:
  `graph.rs`'s unit tests, every persistence/retrieval method + invariant), `bpa-orchd`
  `dispatch_integration` 7→17 (+10: the 9 graph verbs' socket-dispatch + push-fan-out tests),
  `bpa-orchd-proto` `roundtrip` 8→11 (+3) and `ts_export` 11→13 (+2) (the new graph entities/verbs'
  CBOR round-trip + ts-rs parity), `builder-pro-ai` lib 214→217 (+3: the `orchd_graph_*`
  stub-daemon command test + `broker.rs`'s `map_orchd_push` test for `GraphChanged`). One test run
  during this measurement pass hit a transient PTY-resource flake in
  `bpa-sessiond`'s `attach::tests::remove_session_lets_forwarder_drain_then_terminate`
  (`openpty` returned `Os { code: -6 }` under full-workspace parallel test execution — the same
  known category as the documented `natural_exit_final_output_…` PTY flake, BL-40); a clean re-run
  passed all 726 with 0 failures, and this is the number recorded above. Re-run
  `cargo test --workspace -- --list` yourself for the exact current per-crate breakdown — the
  paragraphs below are kept for history and no longer reflect current totals.
- TypeScript (`npx vitest run`): **559 tests**, 35 test files, 0 failed (re-measured this pass).
  Delta vs. the prior S3 pass (502, 33 files): S4 added 2 new test files —
  `components/graph/graphMapping.test.ts` (12 — the pure domain→xyflow mapping helpers, node
  environment, no renderer) and `components/graph/GraphCanvas.test.tsx` (19 — rendered under
  `// @vitest-environment jsdom` + a local `mockReactFlow()` shim, D10) — plus growth in
  `store/store.test.ts` (52→54, `graphByProject`/`refreshGraph`), `ipc/orchd.test.ts` (52→62, the
  9 graph wrapper functions), `ipc/events.test.ts` (20→21, `onOrchdGraphChanged`),
  `components/ProjectPanel.test.tsx` (10→13, the «Граф» 7th tab), and `App.test.tsx` (the
  unconditional `orchd://graph-changed` → `refreshGraph` binding) for the S4 knowledge graph
  (spec §7).
- E2E: `npm run e2e:survive` green, unchanged by S4 (still 6 phases, 0–5, socket-harness variant —
  the sessiond wire is untouched, confirmed above); `npm run e2e:orchd` green, **extended this
  cycle** with a new phase 5 (2×`CreateProject` + 2×`GraphAddNode` + a cross-project `GraphAddEdge`
  → `OrchdShutdown{drain:true}` → relaunch → `GraphListProject` on either project still shows the
  edge + the foreign node as an `external_nodes` ghost — `tests/e2e/orchd-survive.mjs`, S4 spec §8
  — the roadmap DoD proof "a cross-project link survives BOTH projects' restarts"). Phases 0-4
  (project/goal/idea/task CRUD survival + export/import round-trip, S3 spec §12) stay green,
  unchanged.

## Test totals — historical (S3, `[0.4.0]`, 2026-07-14) — superseded above

- Rust workspace (`cargo test --workspace`): **655 tests**, 0 failed (re-measured T21; per-binary
  breakdown from the same run: `bpa-daemon-core` lib 29; `bpa-orchd` lib 158 + `boot_integration`
  4 + `dispatch_integration` 7 + `no_secrets_in_logs` 1; `bpa-orchd-proto` `roundtrip` 8 +
  `ts_export` 11; `bpa-paths` lib 18; `bpa-protocol` lib 1 + `cbor_frame_generic` 7 + `framing` 7 +
  `preamble` 7 + `roundtrip` 8 + `ts_export` 7; `bpa-sessiond` lib 155 + `boot_integration` 4 +
  `no_secrets_in_logs` 1 + `rehydrate_attach` 1 + `skeleton` 1; `builder-pro-ai` lib 214 +
  `capabilities` 5 + `invoke_smoke` 1; every `main.rs`/doc-test binary 0). Delta vs. the prior S2
  pass (384): the S3 daemon-core extraction adds `bpa-daemon-core` (+29, new crate); `bpa-sessiond`
  lib is 155 post-extraction (vs. 167 pre-extraction — the six extracted modules' own unit tests
  MOVED to `bpa-daemon-core`, not lost: daemon-core's 29 + sessiond's 155 together still cover
  every behavior the pre-extraction 167 did, proven by the Phase-1 regression net, S3 spec §12);
  `bpa-orchd`/`bpa-orchd-proto` are wholly new crates (+158+4+7+1+8+11 = +189); `builder-pro-ai`
  lib grew 165 → 214 (+49: `orchd_client.rs`'s error-mapping tests, every `orchd_*` command's
  stub-daemon tests in `commands.rs`, `broker.rs`'s `map_orchd_push`/`map_orchd_conn_state` tests,
  `lib.rs`'s `bring_up_orchd`/status-mapping tests, and `launchd.rs`'s orchd-identity plist test,
  S3 §9). Re-run `cargo test --workspace -- --list` yourself for the exact current per-crate
  breakdown — the paragraphs below are kept for history and no longer reflect current totals.
- TypeScript (`npx vitest run`): **502 tests**, 33 test files, 0 failed (re-measured T21). Delta
  vs. the prior S2 pass (297, 22 files): S3 added 11 new test files — `components/
  ProjectPanel.test.tsx` (10), `components/GoalTree.test.tsx` (9), `components/
  IdeasList.test.tsx` (10), `components/TasksList.test.tsx` (12), `components/
  InsightsList.test.tsx` (7), `components/RulesetPanel.test.tsx` (12), `components/
  CreateProjectDialog.test.tsx` (11), `components/QuickCapture.test.tsx` (13), `components/
  HomeGoals.test.tsx` (8), `components/OrchdDownBanner.test.tsx` (2), `ipc/orchd.test.ts` (52) —
  plus growth in `store/store.test.ts`, `ipc/events.test.ts`, `components/
  WorkspaceSidebar.test.tsx`, `components/UpgradeDialog.test.tsx`, and `App.test.tsx` for the
  `domainSlice`, `orchd://*` events, project-group rail rows, and dual-daemon upgrade dialog
  (S3 §10-§11).
- E2E: `npm run e2e:survive` green, unchanged by S3 (still 6 phases, 0–5, socket-harness variant —
  the sessiond wire is untouched); `npm run e2e:orchd` green, **NEW this cycle** (5 phases, 0-4:
  boot+handshake → create+2goals+idea+task → drain-restart+rehydrate → export → wipe+reimport+
  re-export-equals, `tests/e2e/orchd-survive.mjs`, S3 spec §12 — the roadmap DoD proof).

## Test totals — historical (S2, `[0.3.0]`, 2026-07-09) — superseded above

- Rust workspace (`cargo test --workspace`): **384 tests**, 0 failed. Delta vs. the prior Pv2 pass
  (238): `bpa-paths` grew 7→18 (+11: `validate_path_within`/`validate_parent_within` and their
  escape/parent/final-component edge cases, S2 §4.1); `bpa-protocol` grew (+2 ts_export:
  `workspace_exposes_roots_array`, `command_event_is_exported_camelcase_with_number_seq_and_ts`;
  +2 roundtrip: `workspace_with_multiple_roots_roundtrips_via_cbor`,
  `command_event_roundtrips_via_cbor`); `bpa-sessiond` lib grew 167 (schema v3 migration +
  multi-root persistence + Add/RemoveWorkspaceRoot + GetCommandEvents handlers, S2 §3); the core
  crate (`builder-pro-ai`) lib grew to include the whole new `fs_explorer` (38 tests) and
  `fs_watcher` (21 tests) modules (S2 §4-§5). Re-run `cargo test --workspace -- --list` yourself
  for the exact per-crate breakdown — the paragraphs below are kept for history and no longer
  reflect current totals.
- TypeScript (`npx vitest run`): **297 tests**, 22 test files, 0 failed. Delta vs. the prior Pv2
  pass (118, 13 files): S2 added 9 new test files — `ipc/fs.test.ts` (15), `terminal/
  link-provider.test.ts` (18), `components/HomeView.test.tsx` (12), `components/FileTree.test.tsx`
  (20), `components/FilePreview.test.tsx` (10), `components/FilesRail.test.tsx` (8),
  `components/CommandStrip.test.tsx` (10), plus growth in `store/store.test.ts`, `ipc/
  events.test.ts`, `ipc/commands.test.ts`, and `App.test.tsx` for the fs-slice, workspace-root
  IPC, attention-first navigation, and root-aware stat chips (S2 §6).
- E2E (`npm run e2e:survive`): green, **6 phases** (0–5, socket-harness variant), unchanged by S2
  (the survive-restart property is orthogonal to multi-root/file-explorer additions — additive
  wire changes keep passing through the same harness).

## Test totals — Pv2 (`[0.2.0]`, 2026-07-07) — historical, superseded above

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

`scripts/coverage-gate.sh` runs `cargo llvm-cov --package bpa-sessiond --fail-under-lines 80` AND
(as of S3, `[0.4.0]`; unchanged interface as of S4) `cargo llvm-cov --package bpa-orchd
--fail-under-lines 80` — two real, enforcing gates (either one failing below 80% fails the script).

**`bpa-orchd` — measured (2026-07-14, S4 Task 9 docs-truth gate run): line coverage = 89.74 %**
(regions 87.21 %, functions 90.28 %; 12688 regions/1623 missed, 607 functions/59 missed, 6959
lines/714 missed — up from S3's 87.90 % line coverage, driven by the new `graph.rs` module's own
thoroughly-unit-tested surface). Per-module lines: `boot.rs` 81.65 %, `export.rs` 93.01 %,
`graph.rs` 95.47 % (**new this cycle** — 2940 regions/212 missed, 133 functions/3 missed, 1654
lines/75 missed), `persistence.rs` 95.70 %, `ruleset_files.rs` 97.56 %, `socket_server.rs`
54.53 %, `main.rs` 0 % — the process-concerns entrypoint, never unit-tested, same shape as
sessiond's own `main.rs`; the crate TOTAL clears the 80% gate with headroom, no new tests needed
beyond what S4 already shipped). `socket_server.rs`'s lower per-file number is expected — its
dispatch arms (including the 9 new graph verb arms) are exercised by `dispatch_integration.rs`'s
real-socket integration tests (counted separately by `cargo llvm-cov`, not folded into the
unit-test-only per-file number above) rather than by `--lib` unit tests. *(Historical: the S3/T21
gate-verification run measured 87.90 % line / 85.53 % region / 88.22 % function coverage on the
pre-S4 crate — `boot.rs` 81.65 %, `export.rs` 93.01 %, `persistence.rs` 95.65 %,
`ruleset_files.rs` 97.56 %, `socket_server.rs` 50.32 %, `main.rs` 0 %.)*

**`bpa-sessiond` — measured (2026-07-14, S4 Task 9 docs-truth gate run): line coverage = 90.39 %**
(regions 89.31 %, functions 90.79 %; 12086 regions/1292 missed, 608 functions/56 missed, 7659
lines/736 missed — the gate passes with headroom). This is the EXACT SAME measurement as the prior
S3/T21 run, region-for-region and line-for-line — expected and confirms the S4 spec §2 claim "NO
sessiond change" at the coverage-tooling level, not just by source diff. Per-module lines:
`attach.rs` 88.80 %, `boot.rs` 77.24 %, `live_grid.rs` 93.33 %, `main.rs` 0 % (entrypoint, never
unit-tested), `osc_parser.rs` 94.82 %, `persistence.rs` 94.44 %, `pty_supervisor.rs` 91.46 %,
`scrollback.rs` 93.12 %, `shell_integration/mod.rs` 92.51 %, `singleton.rs` 70.83 % (a thin
wrapper over `bpa-daemon-core::singleton` — most of its former logic, and former coverage,
belongs to daemon-core's own package total now), `socket_server.rs` 90.79 %.
*(Historical, pre-S4: 2026-07-09, S2/`[0.3.0]` cycle measured 89.16 % line / 89.28 % functions /
90.25 % regions; 2026-07-07, Pv2/`[0.2.0]` cycle measured 89.58 % line / 88.17 % functions /
88.65 % regions; 2026-07-05, docs-truth/CI cycle measured 88.06 % line / 86.70 % functions /
89.20 % regions.)*

The gate runs in two enforced places:

- locally as `scripts/final-suite.sh` stage 7/9 (requires
  `rustup component add llvm-tools-preview && cargo install cargo-llvm-cov`);
- in CI as the blocking `coverage` job of `.github/workflows/ci.yml`, step renamed
  `coverage gate (sessiond + orchd >= 80%)` (see `docs/backlog.md` BL-17 — added and verified
  green in the S2 cycle, now covering both daemon crates).

The evidence base behind `bpa-sessiond`'s number: `bpa-sessiond --lib` was 167 tests at S2
measurement time (covering every module: `attach`, `boot`, `live_grid`, `logging`, `osc_parser`,
`persistence`, `pty_supervisor`, `scrollback`, `shell_integration`, `singleton`, `socket_server`)
plus 3 `boot_integration` + 1 `no_secrets_in_logs` + 1 `rehydrate_attach` + 1 `skeleton`
integration tests exercising the full boot→serve→drain (and cold-rehydrate→attach) lifecycle over
the real wire protocol; post-S3-extraction (unchanged through S4) it is 155 lib tests (the six
daemon-core modules' own unit tests moved to `bpa-daemon-core`'s 29, not lost — see "Test totals"
above) plus the same 4 integration-test files. The evidence base behind `bpa-orchd`'s number: 211
`--lib` unit tests (covering `boot`, `export`, `graph` [new, S4], `persistence`, `ruleset_files`,
`socket_server`) plus 4 `boot_integration` + 17 `dispatch_integration` + 1 `no_secrets_in_logs`
integration tests exercising dispatch over a real Unix socket end-to-end.

*(History: at S0+S1 completion this gate was documented but not executed — the authoring
environment lacked the ~3–5 GB the instrumented build needs. That gap was closed by the
docs-truth/CI cycle; the S2 paragraph above records the first real sessiond measurement, and the
S3 paragraph above records the first real orchd measurement.)*
