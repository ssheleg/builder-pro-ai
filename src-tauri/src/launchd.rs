//! Per-user LaunchAgent management, shared by BOTH `bpa-sessiond` (spec §8.3) and `bpa-orchd`
//! (spec §9): `LaunchdAgent` is parameterized by `label`/`stdout_log_name`/`stderr_log_name` (S3
//! T11) so the exact same install/bootstrap/kickstart machinery manages either daemon — only the
//! caller-supplied identity/log names differ, never the logic. launchd owns the daemon lifecycle;
//! the GUI installs the plist, bootstraps it, and kickstarts on demand. All launchctl calls go
//! through an injectable runner so unit tests never mutate the real service database.
//! Degradation: hard failures surface a typed error the UI renders as an actionable banner (spec
//! §13).

use std::path::PathBuf;
use std::process::Command;

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

impl LaunchctlRunner for RealLaunchctl {
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
        let out = Command::new("/bin/launchctl").args(args).output()?;
        Ok(LaunchctlOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
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

/// stderr/exit-code signals launchd already knows this label (idempotent bootstrap).
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

    /// Write the plist (spec §8.3) to `launch_agents_dir`, ensuring the dir exists.
    pub fn install_agent(&self) -> Result<PathBuf, LaunchdError> {
        std::fs::create_dir_all(&self.launch_agents_dir)?;
        std::fs::create_dir_all(self.logs_dir())?;
        let path = self.plist_path();
        std::fs::write(&path, self.render_plist())?;
        tracing::info!(plist = %path.display(), "installed LaunchAgent plist");
        Ok(path)
    }

    /// `launchctl bootstrap gui/<uid> <plist>`; idempotent. A clean load (exit 0) OR
    /// "already bootstrapped" (launchctl exit 5) are BOTH success and must NEVER trigger a
    /// `bootout`.
    ///
    /// Why no bootout on "already": `launchctl bootstrap` on an already-loaded label returns exit 5
    /// regardless of whether the plist changed (it does not diff the on-disk plist against the
    /// loaded one), so exit 5 carries NO drift information. Treating it as drift and running
    /// `bootout` would SIGTERM the healthy running daemon — and every live PTY it owns — on EVERY
    /// app launch, silently voiding the app's core survival promise ("live shells survive the GUI
    /// closing") on the most routine action there is (REL-1, empirical ×3). The bitter irony: the
    /// [`Self::kickstart`] doc forbids `-k` for exactly this reason, while the old bootstrap did the
    /// same kill one call earlier. A plist/binary change that genuinely needs a reload goes through
    /// the consent-gated upgrade flow ([`Self::kickstart_force`], gated by the T10b dialog) — NOT a
    /// blind bootout here (see also BL-34: a stale-but-compatible daemon is intentionally not
    /// force-restarted on the plain boot path).
    pub fn bootstrap(&self) -> Result<(), LaunchdError> {
        let plist = self.plist_path();
        let plist_str = plist.to_string_lossy().into_owned();
        let domain = self.domain_target();

        let out = self.runner.run(&["bootstrap", &domain, &plist_str])?;
        if out.code == 0 || is_already_signal(&out) {
            // Clean load, OR already loaded (the common case on every launch after the first) —
            // both are success. The service is up and may hold live sessions; never bootout.
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
    fn bootstrap_already_bootstrapped_is_success_without_bootout() {
        // "already bootstrapped" (launchctl exit 5) is SUCCESS and must NOT bootout: `launchctl
        // bootstrap` on an already-loaded label returns exit 5 regardless of plist drift (it does
        // not diff), so a bootout here would SIGTERM the healthy running daemon and every live PTY
        // it owns on EVERY app launch — voiding the survival promise (REL-1, empirical ×3). The
        // service is already loaded; that is the goal. A genuine plist/binary reload goes through
        // the consent-gated upgrade flow (`kickstart_force`), not a blind bootout here.
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
            "already-bootstrapped must NOT bootout (REL-1) — exactly one bootstrap call, no bootout"
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
}
