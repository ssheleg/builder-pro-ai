//! Per-user LaunchAgent management for `bpa-sessiond` (spec §8.3).
//! launchd owns the daemon lifecycle; the GUI installs the plist, bootstraps it,
//! and kickstarts on demand. All launchctl calls go through an injectable runner
//! so unit tests never mutate the real service database. Degradation: hard
//! failures surface a typed error the UI renders as an actionable banner (spec §13).

use std::path::PathBuf;
use std::process::Command;

/// Locked identity (spec Global Constraints).
pub const LABEL: &str = "ai.builderpro.desktop.sessiond";

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

/// Per-user LaunchAgent for `bpa-sessiond`: renders the plist, installs it under
/// `launch_agents_dir`, and drives `launchctl` (bootstrap/kickstart/print) through
/// the injectable `runner`. All fields are injectable so unit tests operate purely
/// on temp dirs + a mock runner — never the real `~/Library/LaunchAgents` or `launchctl`.
pub struct LaunchdAgent<'a> {
    pub runner: &'a dyn LaunchctlRunner,
    pub uid: u32,
    /// ~/Library/LaunchAgents (injectable for tests)
    pub launch_agents_dir: PathBuf,
    /// APP_SUPPORT (for log paths in the plist)
    pub app_support_dir: PathBuf,
    /// absolute path to the bundled bpa-sessiond
    pub daemon_path: PathBuf,
    /// RESOLVED_SOCKET_PATH
    pub socket_path: PathBuf,
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
        format!("{LABEL}.plist")
    }

    fn plist_path(&self) -> PathBuf {
        self.launch_agents_dir.join(self.plist_filename())
    }

    fn service_target(&self) -> String {
        format!("gui/{}/{}", self.uid, LABEL)
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
        let out_log = xml_escape(&self.logs_dir().join("sessiond.out.log").to_string_lossy());
        let err_log = xml_escape(&self.logs_dir().join("sessiond.err.log").to_string_lossy());
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
            label = LABEL,
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

    /// `launchctl bootstrap gui/<uid> <plist>`; "already bootstrapped" == success;
    /// on plist drift bootout+re-bootstrap.
    pub fn bootstrap(&self) -> Result<(), LaunchdError> {
        let plist = self.plist_path();
        let plist_str = plist.to_string_lossy().into_owned();
        let domain = self.domain_target();

        let out = self.runner.run(&["bootstrap", &domain, &plist_str])?;
        if out.code == 0 {
            return Ok(());
        }
        if is_already_signal(&out) {
            // Drift: bootout then re-bootstrap once. bootout's own exit code is
            // best-effort (the target may already be gone) — only the retry matters.
            tracing::warn!(stderr = %out.stderr, "service already bootstrapped; rebootstrapping");
            let target = self.service_target();
            let _ = self.runner.run(&["bootout", &target])?;
            let retry = self.runner.run(&["bootstrap", &domain, &plist_str])?;
            if retry.code == 0 || is_already_signal(&retry) {
                return Ok(());
            }
            return Err(LaunchdError::Install(retry.stderr));
        }
        Err(LaunchdError::Install(out.stderr))
    }

    /// `launchctl kickstart gui/<uid>/<label>`.
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

    /// `launchctl print gui/<uid>/<label>` exit 0 => loaded.
    pub fn is_loaded(&self) -> bool {
        let target = self.service_target();
        matches!(self.runner.run(&["print", &target]), Ok(o) if o.code == 0)
    }

    /// Resolve the bundled daemon path from `current_exe()`'s sibling (production helper).
    pub fn resolve_daemon_path() -> Result<PathBuf, LaunchdError> {
        let exe = std::env::current_exe().map_err(|e| LaunchdError::DaemonPath(e.to_string()))?;
        let dir = exe
            .parent()
            .ok_or_else(|| LaunchdError::DaemonPath("current_exe has no parent".into()))?;
        let candidate = dir.join("bpa-sessiond");
        if candidate.exists() {
            Ok(candidate)
        } else {
            Err(LaunchdError::DaemonPath(format!(
                "bpa-sessiond not found beside {}",
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
        // first bootstrap -> "already"; drift path: bootout(ok) then bootstrap(ok)
        let mock = MockLaunchctl::new(vec![already(), ok(), ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        a.bootstrap()
            .expect("already-bootstrapped must be idempotent success");
        let calls = mock.calls();
        assert_eq!(calls[0][0], "bootstrap");
        assert_eq!(calls[0][1], "gui/501");
        assert_eq!(calls[1][0], "bootout");
        assert_eq!(calls[1][1], "gui/501/ai.builderpro.desktop.sessiond");
        assert_eq!(calls[2][0], "bootstrap");
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
}
