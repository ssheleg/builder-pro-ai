//! Keep-awake power assertions (SCN-045, FLW-18): while the keep-awake toggle is ON (default on)
//! AND ≥1 live session exists, hold a macOS `IOPMAssertion` preventing **idle system sleep** so a
//! long agent run survives the owner walking away; release it the moment the last live session
//! ends or the toggle turns off.
//!
//! ## Shape (mirrors `fs_watcher.rs`'s managed-slot pattern)
//!
//! - [`KeepAwakeState`] + [`SleepAsserter`] + [`KeepAwakeState::reconcile`] are the PURE core:
//!   `want = enabled && live_sessions > 0`; acquire when `want && !held`, release when
//!   `!want && held`. No Tauri, no FFI — unit-tested against a mock asserter below.
//! - [`KeepAwake`] is the stateful wrapper the Tauri-managed [`PowerSlot`] holds: it owns the
//!   platform asserter and the last acquire failure, and answers every mutation with a
//!   [`PowerStatus`] snapshot for the webview.
//! - [`IoPmAsserter`] is the ONLY impure part: the macOS IOKit FFI, kept entirely inside this file
//!   (and `#[cfg(target_os = "macos")]`-gated; every other platform gets an honest
//!   `Err("unsupported")` stub — never a silent fake "awake", spec §7 / SCN-045).
//!
//! ## Honesty invariants (SCN-045 "Errors & recovery")
//!
//! - An acquire failure leaves `held == false` and surfaces the OS error string in
//!   [`PowerStatus::error`] — the frontend toasts it, records a Diagnostics event, and flips the
//!   pill to its failure state. `active` is ALWAYS `asserter.is_held()`, never the intent.
//! - A later reconcile with the same want retries the acquire, so a transient OS denial
//!   self-heals on the next session/toggle transition (the recovery test below locks this).
//! - App quit needs no teardown: IOPM assertions are process-scoped — the kernel releases them
//!   automatically when the process exits (cleanly or by crash), so there is deliberately no
//!   exit hook here (SCN-045 "app quit/crash → assertion released by OS, no orphan lock").
//!   [`IoPmAsserter`]'s `Drop` still releases eagerly as in-process hygiene.

use serde::Serialize;
use std::sync::Mutex;

/// Abstraction over "hold a system sleep assertion" so the reconciler is testable without IOKit
/// (mirrors `launchd.rs`'s `LaunchctlRunner` seam). `Send` because the implementor lives inside
/// the Tauri-managed [`PowerSlot`] and commands run on worker threads.
pub trait SleepAsserter: Send {
    /// Idempotently acquire the assertion. `Err(reason)` = the OS denied it — the caller must
    /// stay honest (`held` remains `false`; the reason reaches the UI verbatim).
    fn acquire(&mut self) -> Result<(), String>;
    /// Idempotently release the assertion (no-op when not held).
    fn release(&mut self);
    /// Whether the assertion is currently held — the ONE source of truth `PowerStatus::active`
    /// reports (never the toggle intent).
    fn is_held(&self) -> bool;
}

/// The two inputs the reconcile rule reads (SCN-045): the persisted toggle and the live-session
/// count the frontend syncs down (`lifecycle.kind !== "exited"` sessions, see `App.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAwakeState {
    pub enabled: bool,
    pub live_sessions: usize,
}

impl Default for KeepAwakeState {
    /// Default ON with zero sessions (SCN-045 "default on"): nothing is asserted until the first
    /// live session syncs in, but a fresh install that never touched the toggle keeps machines
    /// awake as soon as an agent runs. The frontend pushes its persisted preference over this at
    /// boot (`App.tsx`), so a persisted "off" wins before any session can exist.
    fn default() -> Self {
        KeepAwakeState {
            enabled: true,
            live_sessions: 0,
        }
    }
}

impl KeepAwakeState {
    /// The pure reconcile rule (SCN-045): drive `asserter` to `want = enabled && live_sessions > 0`.
    /// Returns `Err(reason)` ONLY for an acquire the OS denied — the asserter is then still not
    /// held (honest failure, never a fake "awake"). Release cannot fail. Idempotent: re-running
    /// with an unchanged want is a no-op (no double acquire/release — the idempotency tests below
    /// lock this).
    pub fn reconcile(&self, asserter: &mut dyn SleepAsserter) -> Result<(), String> {
        let want = self.enabled && self.live_sessions > 0;
        if want && !asserter.is_held() {
            // The only fallible transition: an OS denial propagates verbatim (held stays false —
            // the asserter contract) so the caller can surface it honestly, and the next
            // reconcile with an unchanged want naturally retries (the recovery path).
            asserter.acquire()
        } else if !want && asserter.is_held() {
            asserter.release();
            Ok(())
        } else {
            // Already converged (held+wanted, or unheld+unwanted) — idempotent no-op.
            Ok(())
        }
    }
}

/// Webview-facing snapshot (serde camelCase, mirrored by `src/ipc/power.ts::PowerStatus`):
/// `enabled` = the toggle, `active` = the assertion is genuinely held right now,
/// `error` = the most recent acquire denial while one is still wanted-but-unheld (`None`
/// otherwise — including after the want goes away, so a stale denial never lingers on the pill).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerStatus {
    pub enabled: bool,
    pub active: bool,
    pub error: Option<String>,
}

/// The stateful keeper the Tauri-managed [`PowerSlot`] holds: inputs + platform asserter + the
/// last acquire failure. Every mutation reconciles immediately and answers with a fresh
/// [`PowerStatus`], so the webview never has to poll to learn the outcome of its own call.
pub struct KeepAwake {
    state: KeepAwakeState,
    asserter: Box<dyn SleepAsserter>,
    /// The most recent acquire denial while `want` is still true and the assertion unheld;
    /// cleared on a successful acquire AND whenever the want goes away (toggle off / last
    /// session ends) — a stale error must not outlive the condition it described.
    last_error: Option<String>,
}

impl KeepAwake {
    pub fn new(asserter: Box<dyn SleepAsserter>) -> Self {
        KeepAwake {
            state: KeepAwakeState::default(),
            asserter,
            last_error: None,
        }
    }

    /// Set the toggle (SCN-045 UI elements: keep-awake toggle) and reconcile.
    pub fn set_enabled(&mut self, enabled: bool) -> PowerStatus {
        self.state.enabled = enabled;
        self.reconcile_and_status()
    }

    /// Sync the live-session count (App.tsx pushes it on every change) and reconcile.
    pub fn sync_sessions(&mut self, live: usize) -> PowerStatus {
        self.state.live_sessions = live;
        self.reconcile_and_status()
    }

    /// Read-only snapshot — touches nothing, for a pull-based `power_status` fallback (same
    /// rationale as `daemon_status`, finding [12]: never depend solely on the caller having seen
    /// the last mutation's reply).
    pub fn status(&self) -> PowerStatus {
        PowerStatus {
            enabled: self.state.enabled,
            active: self.asserter.is_held(),
            error: self.last_error.clone(),
        }
    }

    fn reconcile_and_status(&mut self) -> PowerStatus {
        self.last_error = self.state.reconcile(self.asserter.as_mut()).err();
        self.status()
    }
}

/// Tauri-managed slot (mirrors `fs_watcher::WatchSlot`): constructed once via [`new_power_slot`]
/// in `lib.rs`'s builder, independent of daemon connectivity (keep-awake is core-local).
pub type PowerSlot = Mutex<KeepAwake>;

/// Construct the managed slot with the platform asserter — call once from `lib.rs`'s
/// `.manage(...)` (mirrors `fs_watcher::new_watch_slot`).
pub fn new_power_slot() -> PowerSlot {
    Mutex::new(KeepAwake::new(new_platform_asserter()))
}

#[cfg(target_os = "macos")]
fn new_platform_asserter() -> Box<dyn SleepAsserter> {
    Box::new(IoPmAsserter::new())
}

#[cfg(not(target_os = "macos"))]
fn new_platform_asserter() -> Box<dyn SleepAsserter> {
    Box::new(UnsupportedAsserter)
}

/// Honest stub for every non-macOS build (SCN-045 §7 honesty: an unsupported platform reports
/// "unsupported" — the pill shows the failure state — instead of pretending to hold anything).
#[cfg(not(target_os = "macos"))]
struct UnsupportedAsserter;

#[cfg(not(target_os = "macos"))]
impl SleepAsserter for UnsupportedAsserter {
    fn acquire(&mut self) -> Result<(), String> {
        Err("keep-awake is unsupported on this platform".to_string())
    }
    fn release(&mut self) {}
    fn is_held(&self) -> bool {
        false
    }
}

// ── macOS IOKit FFI (the ONLY impure code in this module) ───────────────────────────────────────

/// Real macOS sleep assertion via `IOPMAssertionCreateWithName` (IOKit). Assertion type
/// `kIOPMAssertionTypePreventUserIdleSystemSleep` — prevents IDLE sleep only: a lid close or an
/// explicit  → Sleep still sleeps the machine (deliberate; SCN-045 keeps an unattended run
/// alive, it does not fight the owner). Process-scoped: the kernel auto-releases the assertion
/// when this process exits, so app quit/crash leaves no orphan lock (SCN-045
/// "Errors & recovery") — `Drop` below is only eager in-process hygiene, not correctness.
#[cfg(target_os = "macos")]
struct IoPmAsserter {
    /// `Some(id)` while the assertion is held — the id `IOPMAssertionRelease` needs.
    assertion_id: Option<u32>,
}

#[cfg(target_os = "macos")]
mod iopm_ffi {
    //! Minimal hand-rolled bindings (no new crate dependency for two calls — mirrors the repo's
    //! existing bare-`libc` discipline). Signatures per IOKit's `IOPMLib.h` and CoreFoundation's
    //! `CFString.h`; all pointers are opaque.

    use std::os::raw::{c_char, c_void};

    /// Opaque `CFStringRef` (CoreFoundation string handle).
    pub type CFStringRef = *const c_void;
    /// Opaque `CFAllocatorRef`; we only ever pass `kCFAllocatorDefault`.
    pub type CFAllocatorRef = *const c_void;
    /// `IOReturn` (a `kern_return_t`, i.e. C `int`); `0` == `kIOReturnSuccess`.
    pub type IOReturn = i32;

    /// `kCFStringEncodingUTF8` (`CFString.h`).
    pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `kIOPMAssertionLevelOn` (`IOPMLib.h`) — level 255 = assertion is in force.
    pub const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    /// `kIOReturnSuccess`.
    pub const K_IO_RETURN_SUCCESS: IOReturn = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFAllocatorDefault: CFAllocatorRef;
        pub fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        pub fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        pub fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut u32,
        ) -> IOReturn;
        pub fn IOPMAssertionRelease(assertion_id: u32) -> IOReturn;
    }
}

#[cfg(target_os = "macos")]
impl IoPmAsserter {
    /// The IOPM assertion type string (`IOPMLib.h`'s
    /// `kIOPMAssertionTypePreventUserIdleSystemSleep`): prevents idle SYSTEM sleep (display may
    /// still sleep — an unattended agent run needs the CPU, not the screen).
    const ASSERTION_TYPE: &'static str = "PreventUserIdleSystemSleep";
    /// Human-readable assertion name — what the owner sees in `pmset -g assertions` /
    /// Activity Monitor's Energy tab, so the hold is attributable, never mysterious.
    const ASSERTION_NAME: &'static str = "Builder Pro AI — live agent sessions";

    fn new() -> Self {
        IoPmAsserter { assertion_id: None }
    }

    /// Build a `CFStringRef` from a Rust str. `None` only if the string contains an interior NUL
    /// or CF itself fails to allocate — both effectively unreachable for the two `'static`
    /// constants above, but mapped to an honest `Err` rather than an `unwrap` in FFI code.
    fn cf_string(s: &str) -> Option<iopm_ffi::CFStringRef> {
        let c = std::ffi::CString::new(s).ok()?;
        // SAFETY: `c` outlives the call; CFStringCreateWithCString copies the bytes out. The
        // returned CFString follows the Create rule — the caller must CFRelease it.
        let cf = unsafe {
            iopm_ffi::CFStringCreateWithCString(
                iopm_ffi::kCFAllocatorDefault,
                c.as_ptr(),
                iopm_ffi::K_CF_STRING_ENCODING_UTF8,
            )
        };
        if cf.is_null() {
            None
        } else {
            Some(cf)
        }
    }
}

#[cfg(target_os = "macos")]
impl SleepAsserter for IoPmAsserter {
    fn acquire(&mut self) -> Result<(), String> {
        // Idempotent: the reconciler never double-acquires, but a second call must not leak a
        // second assertion id either (defense in depth — same belt-and-braces the reconciler
        // tests assert from the outside).
        if self.assertion_id.is_some() {
            return Ok(());
        }
        let type_cf = Self::cf_string(Self::ASSERTION_TYPE)
            .ok_or_else(|| "could not build the CFString assertion type".to_string())?;
        let name_cf = match Self::cf_string(Self::ASSERTION_NAME) {
            Some(cf) => cf,
            None => {
                // SAFETY: `type_cf` came from the Create rule above; release it on this early exit.
                unsafe { iopm_ffi::CFRelease(type_cf) };
                return Err("could not build the CFString assertion name".to_string());
            }
        };
        let mut id: u32 = 0;
        // SAFETY: both CFStrings are live for the duration of the call; `id` is a valid out-ptr.
        let rc = unsafe {
            iopm_ffi::IOPMAssertionCreateWithName(
                type_cf,
                iopm_ffi::K_IOPM_ASSERTION_LEVEL_ON,
                name_cf,
                &mut id,
            )
        };
        // SAFETY: both were Create-rule references owned by this function.
        unsafe {
            iopm_ffi::CFRelease(type_cf);
            iopm_ffi::CFRelease(name_cf);
        }
        if rc == iopm_ffi::K_IO_RETURN_SUCCESS {
            self.assertion_id = Some(id);
            Ok(())
        } else {
            // Honest denial (SCN-045): surface the raw IOReturn so the toast/Diagnostics record
            // names WHY — never pretend to be awake.
            Err(format!("IOPMAssertionCreateWithName failed (IOReturn {rc:#010x})"))
        }
    }

    fn release(&mut self) {
        if let Some(id) = self.assertion_id.take() {
            // SAFETY: `id` came from a successful IOPMAssertionCreateWithName and is released
            // exactly once (`take()` cleared the slot). A failure here is unactionable (the
            // kernel reclaims the assertion at process exit regardless) — log-and-continue.
            let rc = unsafe { iopm_ffi::IOPMAssertionRelease(id) };
            if rc != iopm_ffi::K_IO_RETURN_SUCCESS {
                tracing::warn!(rc, "IOPMAssertionRelease returned a non-success IOReturn");
            }
        }
    }

    fn is_held(&self) -> bool {
        self.assertion_id.is_some()
    }
}

#[cfg(target_os = "macos")]
impl Drop for IoPmAsserter {
    /// Eager in-process hygiene only: the kernel would auto-release at process exit anyway
    /// (process-scoped assertion, SCN-045 "no orphan lock"), but a dropped asserter (e.g. a
    /// future slot rebuild) must not leave a live assertion behind for the process's remaining
    /// lifetime.
    fn drop(&mut self) {
        self.release();
    }
}

// ── #[tauri::command] surface (SCN-045; registered in lib.rs's generate_handler!) ───────────────

/// Set the keep-awake toggle (the sidebar pill's click) and reconcile immediately. Infallible at
/// the command layer — every failure mode is IN the returned [`PowerStatus`] (`error`), so the
/// frontend's happy path and failure path read the same reply shape.
#[tauri::command]
pub fn power_set_enabled(enabled: bool, power: tauri::State<'_, PowerSlot>) -> PowerStatus {
    power
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_enabled(enabled)
}

/// Sync the live-session count (`App.tsx` calls this whenever the count of
/// `lifecycle.kind !== "exited"` sessions changes) and reconcile immediately.
#[tauri::command]
pub fn power_sync_sessions(live: usize, power: tauri::State<'_, PowerSlot>) -> PowerStatus {
    power
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .sync_sessions(live)
}

/// Pull the current truth without mutating anything — the pull-based fallback mirror of
/// `daemon_status` (finding [12]): a webview that (re)mounts can re-read the pill state instead
/// of trusting a possibly-stale store snapshot.
#[tauri::command]
pub fn power_status(power: tauri::State<'_, PowerSlot>) -> PowerStatus {
    power.lock().unwrap_or_else(|e| e.into_inner()).status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    /// Shared-inner mock (the `KeepAwake` under test OWNS its `Box<dyn SleepAsserter>`, so the
    /// test keeps an `Arc` handle to the same counters — mirrors `lib.rs`'s `MockLaunchctl`
    /// observability pattern). `fail_with: Some(..)` scripts every subsequent acquire to be
    /// denied until cleared.
    #[derive(Default)]
    struct MockInner {
        held: bool,
        acquires: usize,
        releases: usize,
        fail_with: Option<String>,
    }

    #[derive(Clone)]
    struct MockAsserter {
        inner: Arc<StdMutex<MockInner>>,
    }

    impl MockAsserter {
        fn new() -> (Self, Arc<StdMutex<MockInner>>) {
            let inner = Arc::new(StdMutex::new(MockInner::default()));
            (
                MockAsserter {
                    inner: inner.clone(),
                },
                inner,
            )
        }
    }

    impl SleepAsserter for MockAsserter {
        fn acquire(&mut self) -> Result<(), String> {
            let mut i = self.inner.lock().unwrap();
            i.acquires += 1;
            if let Some(reason) = i.fail_with.clone() {
                // A denied acquire leaves held=false — the honesty invariant the reconciler
                // must propagate, never mask.
                return Err(reason);
            }
            i.held = true;
            Ok(())
        }
        fn release(&mut self) {
            let mut i = self.inner.lock().unwrap();
            i.releases += 1;
            i.held = false;
        }
        fn is_held(&self) -> bool {
            self.inner.lock().unwrap().held
        }
    }

    fn keeper() -> (KeepAwake, Arc<StdMutex<MockInner>>) {
        let (mock, inner) = MockAsserter::new();
        (KeepAwake::new(Box::new(mock)), inner)
    }

    // ── reconcile rule (SCN-045: want = enabled && live_sessions > 0) ───────────────────────

    #[test]
    fn default_state_is_enabled_with_zero_sessions() {
        let s = KeepAwakeState::default();
        assert!(s.enabled, "SCN-045: keep-awake defaults ON");
        assert_eq!(s.live_sessions, 0);
    }

    #[test]
    fn disabled_never_acquires_even_with_live_sessions() {
        let (mut k, inner) = keeper();
        let st = k.set_enabled(false);
        assert_eq!(
            st,
            PowerStatus {
                enabled: false,
                active: false,
                error: None
            }
        );
        let st = k.sync_sessions(3);
        assert!(!st.active);
        assert_eq!(inner.lock().unwrap().acquires, 0, "toggle off => no acquire, ever");
    }

    #[test]
    fn enabled_with_zero_sessions_does_not_acquire() {
        let (mut k, inner) = keeper();
        let st = k.sync_sessions(0);
        assert_eq!(
            st,
            PowerStatus {
                enabled: true,
                active: false,
                error: None
            }
        );
        assert_eq!(inner.lock().unwrap().acquires, 0, "no live session => nothing to hold");
    }

    #[test]
    fn first_live_session_acquires_exactly_once() {
        let (mut k, inner) = keeper();
        let st = k.sync_sessions(1);
        assert_eq!(
            st,
            PowerStatus {
                enabled: true,
                active: true,
                error: None
            }
        );
        assert_eq!(inner.lock().unwrap().acquires, 1);
        assert_eq!(inner.lock().unwrap().releases, 0);
    }

    #[test]
    fn resync_while_held_is_idempotent_no_reacquire() {
        let (mut k, inner) = keeper();
        k.sync_sessions(1);
        k.sync_sessions(1); // the same count again (e.g. an unrelated store update)
        let st = k.sync_sessions(5); // more sessions — still ONE assertion, not one per session
        assert!(st.active);
        assert_eq!(
            inner.lock().unwrap().acquires,
            1,
            "reconcile must be idempotent: held + still-wanted => no second acquire"
        );
    }

    #[test]
    fn last_session_ending_releases() {
        let (mut k, inner) = keeper();
        k.sync_sessions(2);
        k.sync_sessions(1); // one still live — keep holding
        assert_eq!(inner.lock().unwrap().releases, 0);
        let st = k.sync_sessions(0); // the LAST live session ended (SCN-045 step 2)
        assert!(!st.active);
        assert_eq!(inner.lock().unwrap().releases, 1);
        assert!(!inner.lock().unwrap().held);
    }

    #[test]
    fn toggle_off_mid_hold_releases() {
        let (mut k, inner) = keeper();
        k.sync_sessions(1);
        assert!(inner.lock().unwrap().held);
        let st = k.set_enabled(false); // the "or user disables the toggle" branch of SCN-045
        assert_eq!(
            st,
            PowerStatus {
                enabled: false,
                active: false,
                error: None
            }
        );
        assert_eq!(inner.lock().unwrap().releases, 1);
    }

    #[test]
    fn released_state_is_idempotent_no_rerelease() {
        let (mut k, inner) = keeper();
        k.sync_sessions(1);
        k.sync_sessions(0);
        k.sync_sessions(0); // idle resync — nothing held, nothing wanted
        k.set_enabled(false);
        assert_eq!(
            inner.lock().unwrap().releases,
            1,
            "reconcile must be idempotent: not-held + not-wanted => no second release"
        );
    }

    // ── honest failure + recovery (SCN-045 "Errors & recovery") ─────────────────────────────

    #[test]
    fn acquire_denial_reports_error_and_stays_inactive() {
        let (mut k, inner) = keeper();
        inner.lock().unwrap().fail_with = Some("os denied".to_string());
        let st = k.sync_sessions(1);
        assert_eq!(
            st,
            PowerStatus {
                enabled: true,
                active: false, // NEVER a fake "awake": held stayed false
                error: Some("os denied".to_string())
            }
        );
        assert!(!inner.lock().unwrap().held);
    }

    #[test]
    fn later_successful_resync_recovers_after_a_denial() {
        let (mut k, inner) = keeper();
        inner.lock().unwrap().fail_with = Some("os denied".to_string());
        assert!(k.sync_sessions(1).error.is_some());
        inner.lock().unwrap().fail_with = None; // the transient OS condition cleared
        let st = k.sync_sessions(1); // want unchanged — reconcile retries the acquire
        assert_eq!(
            st,
            PowerStatus {
                enabled: true,
                active: true,
                error: None
            }
        );
        assert_eq!(inner.lock().unwrap().acquires, 2, "denied once, then retried");
    }

    #[test]
    fn want_going_away_clears_a_stale_denial() {
        let (mut k, inner) = keeper();
        inner.lock().unwrap().fail_with = Some("os denied".to_string());
        assert!(k.sync_sessions(1).error.is_some());
        let st = k.sync_sessions(0); // last session gone — the denial no longer describes anything
        assert_eq!(
            st,
            PowerStatus {
                enabled: true,
                active: false,
                error: None
            }
        );
        assert_eq!(
            inner.lock().unwrap().releases,
            0,
            "nothing was ever held, so nothing to release"
        );
    }

    // ── status/serde contract (mirrored by src/ipc/power.ts) ────────────────────────────────

    #[test]
    fn status_is_a_pure_read() {
        let (mut k, inner) = keeper();
        k.sync_sessions(1);
        let before = inner.lock().unwrap().acquires;
        let st = k.status();
        assert!(st.active);
        assert_eq!(inner.lock().unwrap().acquires, before, "status() must not reconcile");
    }

    #[test]
    fn power_status_serializes_camel_case_with_nullable_error() {
        let ok = PowerStatus {
            enabled: true,
            active: true,
            error: None,
        };
        assert_eq!(
            serde_json::to_string(&ok).unwrap(),
            r#"{"enabled":true,"active":true,"error":null}"#
        );
        let failed = PowerStatus {
            enabled: true,
            active: false,
            error: Some("os denied".to_string()),
        };
        assert_eq!(
            serde_json::to_string(&failed).unwrap(),
            r#"{"enabled":true,"active":false,"error":"os denied"}"#
        );
    }
}
