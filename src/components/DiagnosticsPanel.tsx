// src/components/DiagnosticsPanel.tsx — S-DIAG: renders the diagnostics ring so the operator can
// reconstruct the cause of a failure after its toast is gone. Read-only over the store's `diagEvents`
// plus a "Copy support bundle" (scrubbed JSON) and "Clear". Opened from the sidebar footer.
import { useAppStore } from "../store/store";
import { toSupportBundle } from "../ipc/diag";
import { Dialog, Badge, Button, EmptyState } from "../ui/primitives";

function formatTs(ts: number): string {
  // Locale time is enough for a within-session log; the copyable bundle carries the raw epoch ms.
  const d = new Date(ts);
  return d.toLocaleTimeString(undefined, { hour12: false });
}

export function DiagnosticsPanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const diagEvents = useAppStore((s) => s.diagEvents);
  const clearDiag = useAppStore((s) => s.clearDiag);

  const copyBundle = () => void navigator.clipboard?.writeText(toSupportBundle(diagEvents));

  const footer =
    diagEvents.length > 0 ? (
      <>
        <Button variant="ghost" size="sm" onClick={copyBundle} data-testid="diag-copy">
          Copy support bundle
        </Button>
        <Button variant="danger" size="sm" onClick={clearDiag} data-testid="diag-clear">
          Clear
        </Button>
      </>
    ) : undefined;

  return (
    <Dialog open={open} title="Diagnostics" onClose={onClose} footer={footer} data-testid="diag-panel">
      {diagEvents.length === 0 ? (
        <EmptyState
          title="No errors recorded"
          hint="Failures this session would appear here with their cause. A clean log is a good sign."
          data-testid="diag-empty"
        />
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--sp-2)" }} data-testid="diag-list">
          {diagEvents.map((e) => (
            <div
              key={e.id}
              data-testid="diag-row"
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "var(--sp-1)",
                padding: "var(--sp-2) var(--sp-3)",
                background: "var(--panel-2)",
                borderRadius: "var(--r-md)",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: "var(--sp-2)" }}>
                <span
                  style={{
                    fontFamily: "var(--font-mono)",
                    fontVariantNumeric: "tabular-nums",
                    fontSize: "var(--fs-xs)",
                    color: "var(--muted)",
                  }}
                >
                  {formatTs(e.ts)}
                </span>
                <Badge tone="danger">{e.kind}</Badge>
                <span style={{ fontSize: "var(--fs-xs)", color: "var(--muted)" }}>{e.op}</span>
              </div>
              <div style={{ fontSize: "var(--fs-sm)", color: "var(--ink)" }}>{e.message}</div>
              {e.detail && (
                <pre
                  style={{
                    margin: 0,
                    fontFamily: "var(--font-mono)",
                    fontSize: "var(--fs-xs)",
                    color: "var(--muted)",
                    whiteSpace: "pre-wrap",
                    overflowWrap: "anywhere",
                  }}
                >
                  {e.detail}
                </pre>
              )}
            </div>
          ))}
        </div>
      )}
    </Dialog>
  );
}
