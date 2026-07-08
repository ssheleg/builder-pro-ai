import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { readFilePreview } from "../ipc/fs";
import type { FilePreview as FilePreviewData, FsError } from "../ipc/fs";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Honest message for a rejected `FsError` (spec §7). Deliberately duplicated from
 * `FileTree.tsx`'s copy (same tiny pure function, same vocabulary) rather than imported, so this
 * component has no dependency on that one — see `FileTree.tsx`'s `describeFsError` doc comment. */
function describeFsError(err: unknown): string {
  const e = err as Partial<FsError> | undefined;
  switch (e?.kind) {
    case "notFound":
      return "файл не найден";
    case "permissionDenied":
      return "нет доступа";
    case "outsideRoot":
      return "путь вне корня воркспейса";
    case "tooLarge":
      return "файл слишком большой";
    case "io":
      return e.message ?? "ошибка ввода-вывода";
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
  padding: 8,
  fontSize: 12,
  color: theme.colors.textDim,
  fontFamily: MONO_FONT,
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
        showToast(`Не удалось открыть файл: ${msg}`);
      })
      .finally(() => {
        if (requestRef.current === token) setLoading(false);
      });
  }, [selectedFile, showToast]);

  if (!selectedFile) {
    return <div style={containerStyle}>Выберите файл для просмотра</div>;
  }

  if (loading) {
    return <div style={containerStyle}>Загрузка…</div>;
  }

  if (error !== null) {
    return (
      <div style={{ ...containerStyle, color: theme.colors.statusExited }}>
        {`Не удалось открыть файл: ${error}`}
      </div>
    );
  }

  if (!preview) {
    return <div style={containerStyle} />;
  }

  if (preview.kind === "binary") {
    return <div style={containerStyle}>{`Бинарный файл · ${formatBytes(preview.size)}`}</div>;
  }

  if (preview.kind === "tooLarge") {
    return (
      <div style={containerStyle}>
        {`Файл слишком большой для предпросмотра · ${formatBytes(preview.size)}`}
      </div>
    );
  }

  return (
    <div style={{ height: "100%", minHeight: 0, overflow: "auto" }}>
      {preview.truncated && (
        <div style={{ padding: "4px 8px", fontSize: 11, color: theme.colors.statusWaiting }}>
          Содержимое могло измениться во время чтения — показан неполный результат.
        </div>
      )}
      <pre
        style={{
          margin: 0,
          padding: 8,
          fontSize: 12,
          lineHeight: 1.5,
          color: theme.colors.text,
          fontFamily: MONO_FONT,
          whiteSpace: "pre",
        }}
      >
        {preview.content}
      </pre>
    </div>
  );
}
