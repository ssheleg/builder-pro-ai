import { describe, it, expect } from "vitest";
import pkg from "../../package.json" assert { type: "json" };

describe("scaffold smoke", () => {
  it("pins the locked frontend versions from spec §3", () => {
    expect(pkg.name).toBe("builder-pro-ai");
    expect(pkg.dependencies["@xterm/xterm"]).toBe("6.0.0");
    expect(pkg.dependencies["react"]).toBe("^19.0.0");
    expect(pkg.dependencies["zustand"]).toBe("^5.0.0");
    expect(pkg.dependencies["@tauri-apps/api"]).toBe("^2");
    // Bundling-only + settings/dialog/fs plugins must all be present.
    const deps: Record<string, string> = pkg.dependencies;
    for (const p of [
      "@tauri-apps/plugin-store",
      "@tauri-apps/plugin-dialog",
      "@tauri-apps/plugin-fs",
      "@tauri-apps/plugin-shell",
    ]) {
      expect(deps[p], `${p} missing`).toBe("^2");
    }
  });
});
