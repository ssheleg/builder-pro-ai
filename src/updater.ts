/**
 * Self-update via Tauri's updater plugin (auto-update from GitHub Releases).
 *
 * The endpoint + signing public key live in `src-tauri/tauri.conf.json > plugins.updater`; CI
 * (`release.yml`) uploads the signed `.app.tar.gz` + `.app.tar.gz.sig` + a `latest.json` manifest
 * to each GitHub Release, and the configured endpoint points at `…/releases/latest/download/latest.json`.
 * The updater verifies the bundle against the embedded pubkey before installing — a tampered asset
 * (even one uploaded to a Release) is rejected.
 *
 * Data/settings preservation is automatic: nothing user-owned lives inside the `.app` bundle
 * (domain data in `app_data_dir`/*.db, UI prefs in localStorage, secrets in Keychain, launchd
 * plists rewritten on boot) — the updater only swaps the `.app`. The ONE interruption is live
 * terminals: after install + relaunch, the new GUI reloads both daemons from the new `.app`
 * (`reconcile_daemon_version` in `lib.rs`), which restarts running shells (their scrollback/history
 * is persisted in `bpa.db` and re-attaches). The confirm dialog states this honestly.
 */
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { strings } from "./strings";

/**
 * Ask the configured endpoint whether a newer version than this build is published. Returns the
 * `Update` handle (call `installUpdate` on it) or `null` when up-to-date / unreachable / unsigned.
 * Never throws — an update check must not break app startup.
 */
export async function checkForUpdate(): Promise<Update | null> {
  try {
    return await check();
  } catch (e) {
    console.error("[updater] check failed (non-fatal):", e);
    return null;
  }
}

/**
 * Download + install `update`, then relaunch the app. Progress is logged to the console (a future
 * iteration can surface it via a channel + toast). Relaunch is the natural completion of an install
 * on macOS — the new `.app` only takes effect once the process restarts.
 */
export async function installUpdate(update: Update): Promise<void> {
  let downloaded = 0;
  let total = 0;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? 0;
        console.log(`[updater] downloading ${total} bytes…`);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        if (total > 0) {
          console.log(`[updater] ${Math.round((downloaded / total) * 100)}%`);
        }
        break;
      case "Finished":
        console.log("[updater] download finished — installing + relaunching");
        break;
    }
  });
  await relaunch();
}

/**
 * Startup flow: check once; if a newer version exists, prompt the user; on accept, download +
 * install + relaunch. Idempotent + best-effort — a network/signature failure or a decline is a
 * no-op. Call this once from `App`'s mount effect.
 */
export async function promptAndInstallStartupUpdate(): Promise<void> {
  const update = await checkForUpdate();
  if (update === null) return;
  const notes = (update.body ?? "").trim();
  const message =
    notes.length > 0
      ? strings.updater.availableWithNotes(update.version, notes)
      : strings.updater.available(update.version);
  if (!window.confirm(message)) return;
  try {
    await installUpdate(update);
  } catch (e) {
    // Surface a failure honestly — never a silent no-op after the user accepted.
    console.error("[updater] install failed:", e);
    window.alert(strings.updater.installFailed);
  }
}
