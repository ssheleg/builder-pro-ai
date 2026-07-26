import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { readFilePreview } from "../ipc/fs";
import type { FilePreview as FilePreviewData, FsError } from "../ipc/fs";
import { strings } from "../strings";

/** Honest message for a rejected `FsError` (spec §7). Deliberately duplicated from
 * `FileTree.tsx`'s copy (same tiny pure function, same vocabulary) rather than imported, so this
 * component has no dependency on that one — see `FileTree.tsx`'s `describeFsError` doc comment. */
function describeFsError(err: unknown): string {
  const e = err as Partial<FsError> | undefined;
  switch (e?.kind) {
    case "notFound":
      return strings.errors.fs.notFound;
    case "permissionDenied":
      return strings.errors.fs.noAccess;
    case "outsideRoot":
      return strings.errors.fs.outsideRoot;
    case "tooLarge":
      return strings.errors.fs.tooLarge;
    case "disconnected":
      return strings.errors.fs.disconnected;
    case "io":
      return e.message ?? strings.errors.fs.io;
    default:
      return err instanceof Error ? err.message : String(err);
  }
}

/** Humanized byte size (`12.3 KB` / `4 MB` / ...) — every honest-placeholder card (spec §7) shows
 * the real size, never leaves it as a raw byte count. */
function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = size / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const digits = value >= 10 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

const containerStyle: CSSProperties = {
  height: "100%",
  minHeight: 0,
  overflow: "auto",
  padding: "var(--sp-2)",
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
  fontFamily: "var(--font-mono)",
};

/**
 * Read-only preview pane under `FileTree` (spec §6.4). Pure reader of the store's
 * `selectedFile` — refetches `readFilePreview` whenever it changes. No editing, no syntax
 * highlighting (out of scope, spec §9 YAGNI v1). `binary`/`tooLarge`/error all render an
 * explicit, honest placeholder card (never a truncated read presented as the whole file, spec
 * §7) — an error ALSO fires a toast (never console-only).
 */
export function FilePreview(): JSX.Element {
  const selectedFile = useAppStore((s) => s.selectedFile);
  const showToast = useAppStore((s) => s.showToast);

  const [preview, setPreview] = useState<FilePreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  // Token guard (same pattern as store.ts's toastToken): a fast re-select (A then B) can leave
  // A's readFilePreview resolving AFTER B's — only the LATEST request's own token may still apply
  // its result, so a stale response can never clobber a newer selection's preview.
  const requestRef = useRef(0);

  useEffect(() => {
    if (!selectedFile) {
      setPreview(null);
      setError(null);
      setLoading(false);
      return;
    }
    const token = ++requestRef.current;
    const { root, rel } = selectedFile;
    setLoading(true);
    setPreview(null);
    setError(null);
    readFilePreview(root, rel)
      .then((p) => {
        if (requestRef.current !== token) return;
        setPreview(p);
      })
      .catch((err: unknown) => {
        if (requestRef.current !== token) return;
        const msg = describeFsError(err);
        setError(msg);
        showToast(strings.files.openFileFailed(msg));
      })
      .finally(() => {
        if (requestRef.current === token) setLoading(false);
      });
  }, [selectedFile, showToast]);

  if (!selectedFile) {
    return <div style={containerStyle}>{strings.files.selectFile}</div>;
  }

  if (loading) {
    return <div style={containerStyle}>{strings.files.loading}</div>;
  }

  if (error !== null) {
    return (
      <div style={{ ...containerStyle, color: "var(--danger)" }}>
        {strings.files.openFileFailed(error)}
      </div>
    );
  }

  if (!preview) {
    return <div style={containerStyle} />;
  }

  if (preview.kind === "binary") {
    return <div style={containerStyle}>{strings.files.binaryFile(formatBytes(preview.size))}</div>;
  }

  if (preview.kind === "tooLarge") {
    return (
      <div style={containerStyle}>
        {strings.files.tooLargePreview(formatBytes(preview.size))}
      </div>
    );
  }

  return (
    <div style={{ height: "100%", minHeight: 0, overflow: "auto" }}>
      {preview.truncated && (
        <div
          style={{
            padding: "var(--sp-1) var(--sp-2)",
            fontSize: "var(--fs-xs)",
            color: "var(--warn)",
            background: "var(--warn-weak)",
          }}
        >
          {strings.files.contentMayHaveChanged}
        </div>
      )}
      <pre
        style={{
          margin: 0,
          padding: "var(--sp-2)",
          fontSize: "var(--fs-sm)",
          lineHeight: 1.5,
          color: "var(--ink)",
          fontFamily: "var(--font-mono)",
          whiteSpace: "pre",
        }}
      >
        {preview.content}
      </pre>
    </div>
  );
}
