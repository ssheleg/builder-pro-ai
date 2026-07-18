// src/components/ErrorBoundary.tsx — S-DIAG: catches a React render crash anywhere below it and,
// instead of a white screen (there was NO boundary before), records the cause into the diagnostics
// ring (`recordRenderCrash`) and shows a tokenized recovery card. A class component because
// `getDerivedStateFromError`/`componentDidCatch` have no hook equivalent.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { useAppStore } from "../store/store";
import { Button } from "../ui/primitives";

type Props = {
  children: ReactNode;
  /** Reload action — injectable so tests don't touch `window.location`. */
  onReload?: () => void;
};
type State = { error: Error | null; componentStack: string };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, componentStack: "" };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const componentStack = info.componentStack ?? "";
    this.setState({ componentStack });
    // Record so the cause is reconstructable from the diagnostics panel / console even though the
    // subtree unmounted. getState() (not a hook) is valid here — this is a class component.
    useAppStore.getState().recordRenderCrash(error, componentStack);
  }

  private reload = () => (this.props.onReload ?? (() => window.location.reload()))();

  private copyDetails = () => {
    const { error, componentStack } = this.state;
    const text = `${error?.name ?? "Error"}: ${error?.message ?? ""}\n${componentStack}`;
    void navigator.clipboard?.writeText(text);
  };

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        data-testid="error-boundary"
        role="alert"
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: "var(--sp-3)",
          minHeight: "100vh",
          padding: "var(--sp-6)",
          background: "var(--bg)",
          color: "var(--ink)",
          textAlign: "center",
        }}
      >
        <div style={{ fontSize: "var(--fs-xl)", fontWeight: 600 }}>Something broke</div>
        <div style={{ maxWidth: 460, color: "var(--muted)", fontSize: "var(--fs-md)", lineHeight: 1.5 }}>
          The interface hit an unexpected error and stopped rendering this view. It has been recorded in
          Diagnostics so the cause can be traced. Reloading usually recovers.
        </div>
        <div
          data-testid="error-boundary-message"
          style={{
            maxWidth: 460,
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-sm)",
            color: "var(--danger)",
            background: "var(--danger-weak)",
            border: "1px solid var(--border)",
            borderRadius: "var(--r-md)",
            padding: "var(--sp-2) var(--sp-3)",
            overflowWrap: "anywhere",
          }}
        >
          {error.name}: {error.message}
        </div>
        <div style={{ display: "flex", gap: "var(--sp-2)" }}>
          <Button variant="primary" onClick={this.reload} data-testid="error-boundary-reload">
            Reload app
          </Button>
          <Button variant="ghost" onClick={this.copyDetails} data-testid="error-boundary-copy">
            Copy details
          </Button>
        </div>
      </div>
    );
  }
}
