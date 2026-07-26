import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import pkg from "../../package.json" assert { type: "json" };

/** Recursively collect every non-test .ts/.tsx file under `dir`. */
function sourceFiles(dir: string, acc: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      sourceFiles(full, acc);
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) {
      acc.push(full);
    }
  }
  return acc;
}

describe("scaffold smoke", () => {
  it("pins the locked frontend versions from spec §3", () => {
    expect(pkg.name).toBe("builder-pro-ai");
    expect(pkg.dependencies["@xterm/xterm"]).toBe("6.0.0");
    expect(pkg.dependencies["react"]).toBe("^19.0.0");
    expect(pkg.dependencies["zustand"]).toBe("^5.0.0");
    expect(pkg.dependencies["@tauri-apps/api"]).toBe("^2");
    // Dialog/shell plugins are in active use and stay pinned.
    const deps: Record<string, string> = pkg.dependencies;
    for (const p of ["@tauri-apps/plugin-dialog", "@tauri-apps/plugin-shell"]) {
      expect(deps[p], `${p} missing`).toBe("^2");
    }
    // 2026-07-24 audit (FE-9/FE-5): the store/fs plugins and the search/serialize/web-links
    // xterm addons were dead code — nothing imported them — and the fs/store ACL surface they
    // dragged in was pure risk. They were removed deliberately; assert their ABSENCE so they
    // don't silently come back.
    for (const p of [
      "@tauri-apps/plugin-store",
      "@tauri-apps/plugin-fs",
      "@xterm/addon-search",
      "@xterm/addon-serialize",
      "@xterm/addon-web-links",
    ]) {
      expect(deps[p], `${p} must stay removed (FE-9/FE-5)`).toBeUndefined();
    }
  });

  // Security regression guard (audit: the fs_explorer commands trust a webview-supplied `root`, so
  // the confused-deputy risk is real ONLY if untrusted content can execute in the webview. Today it
  // can't: there is no HTML-injection sink — React escapes all text and xterm renders to its own
  // canvas. Lock that property so a future sink can't silently reopen the vector while
  // `tauri.conf.json` CSP is still null (BL-2). See BL-102.
  // FE-5: one alternative per DOM API that parses a string into the document — JSX props
  // (`dangerouslySetInnerHTML`, `srcDoc`), property assignment (`inner|outerHTML =`, including
  // the `+=` compound form), and method calls (`insertAdjacentHTML`, `document.write[ln]`).
  it("has no HTML-injection sink in the frontend (dangerouslySetInnerHTML / inner|outerHTML / insertAdjacentHTML / document.write / srcDoc)", () => {
    const SINK_RE =
      /dangerouslySetInnerHTML|\.(?:inner|outer)HTML\s*\+?=|\.insertAdjacentHTML\s*\(|\bdocument\.write(?:ln)?\s*\(|\bsrcDoc\s*=/;
    const here = dirname(fileURLToPath(import.meta.url));
    const srcRoot = dirname(here); // src/
    const offenders: string[] = [];
    for (const file of sourceFiles(srcRoot)) {
      const text = readFileSync(file, "utf8");
      if (SINK_RE.test(text)) {
        offenders.push(file);
      }
    }
    expect(offenders, `HTML-injection sink(s) introduced: ${offenders.join(", ")}`).toEqual([]);
  });
});
