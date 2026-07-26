//! Per-user LaunchAgent management, shared by BOTH `bpa-sessiond` (spec §8.3) and `bpa-orchd`
//! (spec §9): `LaunchdAgent` is parameterized by `label`/`stdout_log_name`/`stderr_log_name` (S3
//! T11) so the exact same install/bootstrap/kickstart machinery manages either daemon — only the
//! caller-supplied identity/log names differ, never the logic. launchd owns the daemon lifecycle;
//! the GUI installs the plist, bootstraps it, and kickstarts on demand. All launchctl calls go
//! through an injectable runner so unit tests never mutate the real service database.
//! Degradation: hard failures surface a typed error the UI renders as an actionable banner (spec
//! §13).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Locked identity (spec Global Constraints). Kept as the sessiond call site's canonical value
/// (`lib.rs::build_launchd_agent`) — the parameterization (S3 T11) is purely additive, so this
/// const's value, and every plist/service-target it produces, are unchanged.
pub const LABEL: &str = "ai.builderpro.desktop.sessiond";
/// orchd's LaunchAgent label (spec §9, S3 T11) — the second daemon this same `LaunchdAgent`
/// machinery now manages.
pub const ORCHD_LABEL: &str = "ai.builderpro.desktop.orchd";

/// Result of a single `launchctl <args...>` invocation.
#[derive(Debug, Clone)]
pub struct LaunchctlOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Injectable launchctl runner so unit tests never touch the real service DB.
pub trait LaunchctlRunner: Send + Sync {
    /// Run `launchctl <args...>`; return the (exit_code, stdout, stderr) triple.
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput>;
}

/// Real runner used in production; shells out to `/bin/launchctl`.
pub struct RealLaunchctl;

/// REL-7 (audit 2026-07-24): hard bound on any single `launchctl` invocation. A hung `launchctl`
/// (wedged service manager, blocked XPC round-trip) previously wedged the GUI's boot path
/// forever — `Command::output()` waits unboundedly. 15 s is generous for any real
/// bootstrap/kickstart/print while turning a hang into a bounded, typed boot failure (spec §13's
/// actionable banner) instead of an infinite one.
const LAUNCHCTL_TIMEOUT: Duration = Duration::from_secs(15);

/// Poll interval for the child-exit watch inside [`run_command_with_timeout`].
const LAUNCHCTL_POLL: Duration = Duration::from_millis(25);

impl LaunchctlRunner for RealLaunchctl {
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
        run_command_with_timeout("/bin/launchctl", args, LAUNCHCTL_TIMEOUT)
    }
}

/// Run `prog <args...>` capturing stdout/stderr, bounded by `timeout` (REL-7). On expiry the
/// child is killed and reaped (no zombie/orphan process left behind) and the call returns
/// `ErrorKind::TimedOut`. No new dependencies: `Child::try_wait` + a short sleep poll — the
/// added ≤[`LAUNCHCTL_POLL`] latency per call is negligible next to launchd's own round-trip,
/// and launchctl's output is always far below the pipe buffer, so the collect-after-exit read
/// can never deadlock on a full pipe.
fn run_command_with_timeout(
    prog: &str,
    args: &[&str],
    timeout: Duration,
) -> std::io::Result<LaunchctlOutput> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            // Exited: pipes are at EOF, and `wait()` reuses the cached exit status.
            let out = child.wait_with_output()?;
            return Ok(LaunchctlOutput {
                code: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait(); // reap: never leave a zombie behind
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("{prog} {args:?} did not exit within {timeout:?}"),
            ));
        }
        std::thread::sleep(LAUNCHCTL_POLL);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchdError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not install background service: {0}")]
    Install(String),
    #[error("launchctl {op} failed (code {code}): {stderr}")]
    Command {
        op: String,
        code: i32,
        stderr: String,
    },
    #[error("cannot resolve daemon path: {0}")]
    DaemonPath(String),
}

/// Per-user LaunchAgent for a single daemon (`bpa-sessiond` OR `bpa-orchd`, spec §8.3/§9): renders
/// the plist, installs it under `launch_agents_dir`, and drives `launchctl`
/// (bootstrap/kickstart/print) through the injectable `runner`. All fields are injectable so unit
/// tests operate purely on temp dirs + a mock runner — never the real `~/Library/LaunchAgents` or
/// `launchctl`.
///
/// `label`/`stdout_log_name`/`stderr_log_name` (S3 T11, added ADDITIVELY) are what let the exact
/// same struct/methods manage either daemon: the sessiond call site (`lib.rs::
/// build_launchd_agent`) passes [`LABEL`]/`"sessiond.out.log"`/`"sessiond.err.log"` — the same
/// values that were hardcoded before this parameterization, so the rendered sessiond plist and
/// service target are byte-identical to pre-T11 — while the orchd call site (`lib.rs::
/// build_orchd_launchd_agent`) passes [`ORCHD_LABEL`]/`"orchd.out.log"`/`"orchd.err.log"`.
pub struct LaunchdAgent<'a> {
    pub runner: &'a dyn LaunchctlRunner,
    pub uid: u32,
    /// ~/Library/LaunchAgents (injectable for tests)
    pub launch_agents_dir: PathBuf,
    /// APP_SUPPORT (for log paths in the plist)
    pub app_support_dir: PathBuf,
    /// absolute path to the bundled daemon binary (`bpa-sessiond` or `bpa-orchd`)
    pub daemon_path: PathBuf,
    /// RESOLVED_SOCKET_PATH
    pub socket_path: PathBuf,
    /// launchd Label + plist filename + service-target suffix (spec Global Constraints / §9):
    /// `LABEL` for sessiond, `ORCHD_LABEL` for orchd.
    pub label: &'static str,
    /// `StandardOutPath` leaf filename under `app_support_dir/logs/`.
    pub stdout_log_name: &'static str,
    /// `StandardErrorPath` leaf filename under `app_support_dir/logs/`.
    pub stderr_log_name: &'static str,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// stderr/exit-code signals launchd already knows this label (idempotent bootstrap). This is a
/// SUCCESS signal, never a drift trigger: an already-bootstrapped service means the daemon is
/// loaded (and possibly RUNNING live sessions) — the boot path must not touch it (REL-1, audit
/// 2026-07-24). Plist drift on an already-loaded service is the upgrade flow's job
/// (`upgrade_daemon` → [`LaunchdAgent::kickstart_force`] behind the consent dialog), not boot's.
fn is_already_signal(out: &LaunchctlOutput) -> bool {
    let s = out.stderr.to_ascii_lowercase();
    out.code == 5 || s.contains("already")
}

/// stderr signals the service is already running (kickstart idempotency).
fn is_already_running(out: &LaunchctlOutput) -> bool {
    let s = out.stderr.to_ascii_lowercase();
    s.contains("already running") || s.contains("service is already running")
}

impl<'a> LaunchdAgent<'a> {
    fn plist_filename(&self) -> String {
        format!("{}.plist", self.label)
    }

    fn plist_path(&self) -> PathBuf {
        self.launch_agents_dir.join(self.plist_filename())
    }

    fn service_target(&self) -> String {
        format!("gui/{}/{}", self.uid, self.label)
    }

    fn domain_target(&self) -> String {
        format!("gui/{}", self.uid)
    }

    fn logs_dir(&self) -> PathBuf {
        self.app_support_dir.join("logs")
    }

    /// Render the plist XML (spec §8.3) — pure, testable.
    pub fn render_plist(&self) -> String {
        let daemon = xml_escape(&self.daemon_path.to_string_lossy());
        let socket = xml_escape(&self.socket_path.to_string_lossy());
        let out_log = xml_escape(&self.logs_dir().join(self.stdout_log_name).to_string_lossy());
        let err_log = xml_escape(&self.logs_dir().join(self.stderr_log_name).to_string_lossy());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{daemon}</string>
    <string>--socket</string>
    <string>{socket}</string>
  </array>
  <key>KeepAlive</key>
  <dict>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>RunAtLoad</key>
  <false/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{out_log}</string>
  <key>StandardErrorPath</key>
  <string>{err_log}</string>
</dict>
</plist>
"#,
            label = self.label,
            daemon = daemon,
            socket = socket,
            out_log = out_log,
            err_log = err_log,
        )
    }

    /// Write the plist (spec §8.3) to `launch_agents_dir`, ensuring the dir exists. The write is
    /// ATOMIC (REL-7, audit 2026-07-24): the plist goes to a temp sibling first and is then
    /// `rename(2)`d over the target — atomic within one filesystem on macOS — so a crash/kill
    /// mid-write can never leave a TRUNCATED plist behind for the next bootstrap to load
    /// half-parsed (a corrupt LaunchAgent is strictly worse than a missing one).
    pub fn install_agent(&self) -> Result<PathBuf, LaunchdError> {
        std::fs::create_dir_all(&self.launch_agents_dir)?;
        std::fs::create_dir_all(self.logs_dir())?;
        let path = self.plist_path();
        let tmp = self
            .launch_agents_dir
            .join(format!(".{}.tmp", self.plist_filename()));
        std::fs::write(&tmp, self.render_plist())?;
        std::fs::rename(&tmp, &path)?;
        tracing::info!(plist = %path.display(), "installed LaunchAgent plist");
        Ok(path)
    }

    /// `launchctl bootstrap gui/<uid> <plist>`; "already bootstrapped" == success, FULL STOP.
    ///
    /// REL-1 (audit 2026-07-24): the pre-fix code treated the "already" signal as plist drift and
    /// ran `bootout` + re-bootstrap — but `bootout` on an already-loaded service KILLS the running
    /// daemon (and every live session it holds), so every single GUI launch destroyed the very
    /// daemon it was about to connect to. An already-loaded service is exactly what a healthy
    /// second launch looks like, so it is plain idempotent success here. Genuine plist drift (old
    /// daemon binary loaded from an older install) is reconciled by the upgrade flow
    /// (`upgrade_daemon` → [`Self::kickstart_force`]), which is consent-gated precisely because it
    /// restarts the daemon — never by the boot path.
    pub fn bootstrap(&self) -> Result<(), LaunchdError> {
        let plist = self.plist_path();
        let plist_str = plist.to_string_lossy().into_owned();
        let domain = self.domain_target();

        let out = self.runner.run(&["bootstrap", &domain, &plist_str])?;
        if out.code == 0 || is_already_signal(&out) {
            return Ok(());
        }
        Err(LaunchdError::Install(out.stderr))
    }

    /// `launchctl kickstart gui/<uid>/<label>` (no `-k`): idempotent ensure-running. If the
    /// service is already up, this is a no-op that leaves the running process (and every live
    /// session it holds) completely untouched — this is the ONLY kickstart variant the boot path
    /// (`ensure_daemon_running`) may call. Using `-k` here would force-kill a running daemon on
    /// every single app launch, destroying every live session with zero consent (finding
    /// [10]/[16] of the final-review wave) and bypassing the upgrade consent dialog before the
    /// handshake even gets a chance to raise `IncompatibleDaemon`. See [`Self::kickstart_force`]
    /// for the force-restart variant used by the upgrade flow.
    ///
    /// The same "boot never kills" rule drives [`Self::bootstrap`]'s REL-1 semantics: its
    /// "already bootstrapped" signal is plain idempotent success (no `bootout`), and any plist
    /// drift on an already-loaded service is reconciled HERE — by the consent-gated upgrade flow
    /// (`upgrade_daemon` → [`Self::kickstart_force`]) — never by the boot path.
    pub fn kickstart(&self) -> Result<(), LaunchdError> {
        let target = self.service_target();
        let out = self.runner.run(&["kickstart", &target])?;
        if out.code == 0 || is_already_running(&out) {
            return Ok(());
        }
        Err(LaunchdError::Command {
            op: "kickstart".into(),
            code: out.code,
            stderr: out.stderr,
        })
    }

    /// `launchctl kickstart -k gui/<uid>/<label>`. `-k` force-kills-then-restarts a running
    /// service (rather than being a no-op when the service is already up) — required so the
    /// upgrade flow (spec §6.2) can force a stale, already-running old-version daemon to relaunch
    /// with the new bundled binary; a plain `kickstart` without `-k` would leave the old process
    /// running untouched. ONLY `upgrade_daemon_core` (commands.rs) may call this — it is gated
    /// behind the T10b consent dialog precisely because it destroys every live session on the old
    /// daemon. The boot path (`ensure_daemon_running`) must use [`Self::kickstart`] instead.
    pub fn kickstart_force(&self) -> Result<(), LaunchdError> {
        let target = self.service_target();
        let out = self.runner.run(&["kickstart", "-k", &target])?;
        if out.code == 0 || is_already_running(&out) {
            return Ok(());
        }
        Err(LaunchdError::Command {
            op: "kickstart".into(),
            code: out.code,
            stderr: out.stderr,
        })
    }

    /// `launchctl print gui/<uid>/<label>` exit 0 => loaded.
    pub fn is_loaded(&self) -> bool {
        let target = self.service_target();
        matches!(self.runner.run(&["print", &target]), Ok(o) if o.code == 0)
    }

    /// Resolve a bundled daemon's path from `current_exe()`'s sibling (production helper).
    /// `bin_name` (S3 T11, additive param) is the sidecar binary's leaf name — `"bpa-sessiond"` or
    /// `"bpa-orchd"`; the `current_exe`-sibling resolution rule itself is unchanged.
    pub fn resolve_daemon_path(bin_name: &str) -> Result<PathBuf, LaunchdError> {
        let exe = std::env::current_exe().map_err(|e| LaunchdError::DaemonPath(e.to_string()))?;
        let dir = exe
            .parent()
            .ok_or_else(|| LaunchdError::DaemonPath("current_exe has no parent".into()))?;
        let candidate = dir.join(bin_name);
        if candidate.exists() {
            Ok(candidate)
        } else {
            Err(LaunchdError::DaemonPath(format!(
                "{bin_name} not found beside {}",
                exe.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Records every launchctl invocation and returns scripted outputs in order.
    struct MockLaunchctl {
        calls: Mutex<RefCell<Vec<Vec<String>>>>,
        scripted: Mutex<RefCell<std::collections::VecDeque<LaunchctlOutput>>>,
    }
    impl MockLaunchctl {
        fn new(outputs: Vec<LaunchctlOutput>) -> Self {
            MockLaunchctl {
                calls: Mutex::new(RefCell::new(Vec::new())),
                scripted: Mutex::new(RefCell::new(outputs.into_iter().collect())),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().borrow().clone()
        }
    }
    impl LaunchctlRunner for MockLaunchctl {
        fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
            self.calls
                .lock()
                .unwrap()
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            let out = self
                .scripted
                .lock()
                .unwrap()
                .borrow_mut()
                .pop_front()
                .unwrap_or(LaunchctlOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            Ok(out)
        }
    }

    fn ok() -> LaunchctlOutput {
        LaunchctlOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
    fn already() -> LaunchctlOutput {
        LaunchctlOutput {
            code: 5,
            stdout: String::new(),
            stderr: "Bootstrap failed: 5: Input/output error (service already bootstrapped)".into(),
        }
    }

    fn agent<'a>(runner: &'a dyn LaunchctlRunner, root: &std::path::Path) -> LaunchdAgent<'a> {
        LaunchdAgent {
            runner,
            uid: 501,
            launch_agents_dir: root.join("LaunchAgents"),
            app_support_dir: root.join("AppSupport"),
            daemon_path: PathBuf::from(
                "/Applications/Builder Pro AI.app/Contents/MacOS/bpa-sessiond",
            ),
            socket_path: PathBuf::from("/tmp/bpa-501/d.sock"),
            label: LABEL,
            stdout_log_name: "sessiond.out.log",
            stderr_log_name: "sessiond.err.log",
        }
    }

    /// Same shape as `agent()`, but for the orchd identity (S3 T11) — proves the
    /// parameterization is a genuine parameter, not a disguised sessiond-only constant.
    fn orchd_agent<'a>(
        runner: &'a dyn LaunchctlRunner,
        root: &std::path::Path,
    ) -> LaunchdAgent<'a> {
        LaunchdAgent {
            runner,
            uid: 501,
            launch_agents_dir: root.join("LaunchAgents"),
            app_support_dir: root.join("AppSupport"),
            daemon_path: PathBuf::from("/Applications/Builder Pro AI.app/Contents/MacOS/bpa-orchd"),
            socket_path: PathBuf::from("/tmp/bpa-501/orchd.sock"),
            label: ORCHD_LABEL,
            stdout_log_name: "orchd.out.log",
            stderr_log_name: "orchd.err.log",
        }
    }

    #[test]
    fn render_plist_has_locked_keys() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        let plist = a.render_plist();
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("<string>ai.builderpro.desktop.sessiond</string>"));
        assert!(plist.contains("<string>--socket</string>"));
        assert!(plist.contains("<string>/tmp/bpa-501/d.sock</string>"));
        // KeepAlive MUST be a dict {Crashed:true}, never bare <true/>
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>Crashed</key>"));
        assert!(!plist.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>ThrottleInterval</key>"));
        assert!(plist.contains("<integer>10</integer>"));
        assert!(plist.contains("<string>Background</string>"));
        assert!(plist.contains("sessiond.out.log"));
        assert!(plist.contains("sessiond.err.log"));
    }

    /// Locked golden-string test (S3 T11, spec §9): `LaunchdAgent` gained `label`/
    /// `stdout_log_name`/`stderr_log_name` fields (previously hardcoded `LABEL`/
    /// `"sessiond.out.log"`/`"sessiond.err.log"` baked directly into `render_plist`'s template) —
    /// the sessiond call site must still render BYTE-IDENTICAL output. `golden` is written
    /// independently of the (now-parameterized) `render_plist()`, reconstructing exactly what the
    /// pre-T11 hardcoded template produced for the same inputs, so this test actually proves
    /// parity rather than tautologically comparing the function to itself.
    #[test]
    fn render_plist_is_byte_identical_to_pre_parameterization_output_for_sessiond() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());

        let out_log = a.app_support_dir.join("logs").join("sessiond.out.log");
        let err_log = a.app_support_dir.join("logs").join("sessiond.err.log");
        let golden = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>ai.builderpro.desktop.sessiond</string>
  <key>ProgramArguments</key>
  <array>
    <string>{daemon}</string>
    <string>--socket</string>
    <string>{socket}</string>
  </array>
  <key>KeepAlive</key>
  <dict>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>RunAtLoad</key>
  <false/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{out_log}</string>
  <key>StandardErrorPath</key>
  <string>{err_log}</string>
</dict>
</plist>
"#,
            daemon = a.daemon_path.to_string_lossy(),
            socket = a.socket_path.to_string_lossy(),
            out_log = out_log.to_string_lossy(),
            err_log = err_log.to_string_lossy(),
        );

        assert_eq!(
            a.render_plist(),
            golden,
            "sessiond's rendered plist must not change one byte across the T11 parameterization"
        );
    }

    /// Proves the parameterization is a genuine parameter (not a disguised sessiond-only
    /// constant): the orchd agent's rendered plist/service-target must use ITS OWN label and log
    /// names, never sessiond's.
    #[test]
    fn render_plist_and_service_target_use_orchd_identity_for_orchd_agent() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = orchd_agent(&mock, tmp.path());

        let plist = a.render_plist();
        assert!(plist.contains("<string>ai.builderpro.desktop.orchd</string>"));
        assert!(!plist.contains("ai.builderpro.desktop.sessiond"));
        assert!(plist.contains("orchd.out.log"));
        assert!(plist.contains("orchd.err.log"));
        assert!(!plist.contains("sessiond.out.log"));
        assert!(!plist.contains("sessiond.err.log"));

        let plist_path = a.install_agent().unwrap();
        assert!(plist_path.ends_with("ai.builderpro.desktop.orchd.plist"));

        a.bootstrap().unwrap();
        assert_eq!(mock.calls()[0][0], "bootstrap");
        assert_eq!(mock.calls()[0][1], "gui/501");
        a.kickstart().unwrap();
        assert_eq!(
            mock.calls()[1],
            vec!["kickstart", "gui/501/ai.builderpro.desktop.orchd"]
        );
    }

    #[test]
    fn install_creates_dirs_and_writes_plist() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        let plist_path = a.install_agent().unwrap();
        assert!(plist_path.ends_with("ai.builderpro.desktop.sessiond.plist"));
        assert!(plist_path.exists(), "plist file written");
        assert!(
            tmp.path().join("AppSupport/logs").is_dir(),
            "log dir created"
        );
        let contents = std::fs::read_to_string(&plist_path).unwrap();
        assert!(contents.contains("ai.builderpro.desktop.sessiond"));
    }

    #[test]
    fn bootstrap_already_bootstrapped_is_success() {
        // REL-1 (audit 2026-07-24): "already bootstrapped" is plain idempotent success — the
        // pre-fix drift path ran `bootout` (which KILLS the running daemon and every live
        // session it holds) on every GUI launch. Now: exactly ONE launchctl call, never bootout.
        let mock = MockLaunchctl::new(vec![already()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        a.bootstrap()
            .expect("already-bootstrapped must be idempotent success");
        let calls = mock.calls();
        assert_eq!(
            calls.len(),
            1,
            "already-bootstrapped must NOT trigger bootout/re-bootstrap, got {calls:?}"
        );
        assert_eq!(calls[0][0], "bootstrap");
        assert_eq!(calls[0][1], "gui/501");
    }

    #[test]
    fn bootstrap_clean_success_no_bootout() {
        let mock = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        a.bootstrap().unwrap();
        assert_eq!(mock.calls().len(), 1, "clean bootstrap must not bootout");
        assert_eq!(mock.calls()[0][0], "bootstrap");
    }

    #[test]
    fn kickstart_cmd_shape() {
        // Boot-path kickstart must NEVER carry `-k`: `-k` force-kills a running daemon, which on
        // the boot path (called on EVERY app launch by ensure_daemon_running) would destroy every
        // live session with zero consent (findings [10]/[16]). Plain `kickstart` is idempotent:
        // a no-op if already running, starts it if not.
        let mock = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.kickstart().unwrap();
        let calls = mock.calls();
        assert_eq!(
            calls[0],
            vec!["kickstart", "gui/501/ai.builderpro.desktop.sessiond"]
        );
    }

    #[test]
    fn kickstart_force_cmd_shape() {
        // The FORCE variant (`-k`) is reserved for the upgrade flow, which is gated behind the
        // T10b consent dialog — it must remain a distinct method from the boot-path `kickstart()`.
        let mock = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.kickstart_force().unwrap();
        let calls = mock.calls();
        assert_eq!(
            calls[0],
            vec!["kickstart", "-k", "gui/501/ai.builderpro.desktop.sessiond"]
        );
    }

    #[test]
    fn kickstart_already_running_is_idempotent_success() {
        let already_running = LaunchctlOutput {
            code: 1,
            stdout: String::new(),
            stderr: "Service is already running".into(),
        };
        let mock = MockLaunchctl::new(vec![already_running]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.kickstart()
            .expect("already-running must be idempotent success for the non-force kickstart");
    }

    #[test]
    fn hard_failure_surfaces_install_error() {
        let boom = LaunchctlOutput {
            code: 78,
            stdout: String::new(),
            stderr: "Operation not permitted (TCC)".into(),
        };
        let mock = MockLaunchctl::new(vec![boom]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        let err = a.bootstrap().unwrap_err();
        match err {
            LaunchdError::Install(msg) => assert!(msg.contains("Operation not permitted")),
            o => panic!("expected Install error, got {o:?}"),
        }
    }

    #[test]
    fn is_loaded_reads_print_exit_code() {
        let loaded = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&loaded, tmp.path());
        assert!(a.is_loaded());

        let unloaded = MockLaunchctl::new(vec![LaunchctlOutput {
            code: 113,
            stdout: String::new(),
            stderr: "Could not find service".into(),
        }]);
        let a2 = agent(&unloaded, tmp.path());
        assert!(!a2.is_loaded());
        assert_eq!(
            unloaded.calls()[0],
            vec!["print", "gui/501/ai.builderpro.desktop.sessiond"]
        );
    }

    #[test]
    fn resolve_daemon_path_uses_the_given_bin_name() {
        // Can't mock `current_exe()` itself, but a bin name that's guaranteed not to exist beside
        // the test binary proves `bin_name` (S3 T11's additive param) actually drives which
        // sibling filename is looked up and quoted back in the error, not a leftover hardcoded
        // "bpa-sessiond".
        let err =
            crate::launchd::LaunchdAgent::resolve_daemon_path("definitely-not-a-real-binary-xyz")
                .unwrap_err();
        match err {
            LaunchdError::DaemonPath(msg) => {
                assert!(
                    msg.contains("definitely-not-a-real-binary-xyz"),
                    "expected the error to name the requested bin_name, got: {msg}"
                );
            }
            o => panic!("expected DaemonPath error, got {o:?}"),
        }
    }

    // ---- REL-7 (audit 2026-07-24): bounded launchctl + atomic plist install. ----

    #[test]
    fn run_command_with_timeout_collects_output_and_exit_code() {
        let out = run_command_with_timeout("/bin/echo", &["hi"], Duration::from_secs(5)).unwrap();
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "hi\n");
    }

    #[test]
    fn run_command_with_timeout_propagates_a_nonzero_exit() {
        let out = run_command_with_timeout("/usr/bin/false", &[], Duration::from_secs(5)).unwrap();
        assert_eq!(out.code, 1);
    }

    #[test]
    fn run_command_with_timeout_kills_a_hung_child_and_returns_timed_out() {
        let start = Instant::now();
        let err = run_command_with_timeout("/bin/sleep", &["30"], Duration::from_millis(150))
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut, "got {err:?}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the call must return promptly after the timeout, not wait out the child"
        );
        // The killed child was reaped via `wait()` — nothing to assert directly without a pid
        // handle, but a zombie would surface as a leaked `<defunct>` under load; the kill+wait
        // ordering is covered by the prompt-return assertion above.
    }

    #[test]
    fn install_agent_is_atomic_no_tmp_left_and_replaces_stale_plist() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());

        let plist_path = a.install_agent().unwrap();
        // Simulate a stale/corrupt install from an earlier crashed run.
        std::fs::write(&plist_path, b"GARBAGE-TRUNCATED").unwrap();

        let plist_path = a.install_agent().unwrap();
        let contents = std::fs::read_to_string(&plist_path).unwrap();
        assert!(
            contents.contains("ai.builderpro.desktop.sessiond"),
            "re-install must atomically REPLACE the stale plist, got: {contents}"
        );
        assert!(
            !contents.contains("GARBAGE-TRUNCATED"),
            "stale bytes must be fully gone after the atomic replace"
        );
        let leftovers: Vec<_> = std::fs::read_dir(a.launch_agents_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp-install sibling may survive install_agent, got {leftovers:?}"
        );
    }
}
