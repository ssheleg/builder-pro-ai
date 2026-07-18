// src/ui/primitives.tsx — S-UXR B2 primitives kit (direction A). Small, token-only building blocks
// (every color/space/radius/type value is a var(--…) from tokens.css) so the whole UI reads as one
// system in both light and dark. No external UI/icon/chart deps — sparklines are inline SVG.
import {
  type CSSProperties,
  type ReactNode,
  type ButtonHTMLAttributes,
  useEffect,
} from "react";
import { statusTone, type Tone } from "./theme";

// ---- tone → color helpers -------------------------------------------------------------------

const TONE_FG: Record<Tone, string> = {
  ink: "var(--ink)",
  muted: "var(--muted)",
  accent: "var(--accent)",
  info: "var(--info)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
};
const TONE_WEAK_BG: Record<Tone, string> = {
  ink: "var(--panel-2)",
  muted: "var(--panel-2)",
  accent: "var(--accent-weak)",
  info: "var(--info-weak)",
  ok: "var(--ok-weak)",
  warn: "var(--warn-weak)",
  danger: "var(--danger-weak)",
};

// ---- Panel ----------------------------------------------------------------------------------

export function Panel({
  title,
  actions,
  padded = true,
  children,
  style,
  ...rest
}: {
  title?: ReactNode;
  actions?: ReactNode;
  padded?: boolean;
  children: ReactNode;
  style?: CSSProperties;
} & Omit<React.HTMLAttributes<HTMLDivElement>, "title">) {
  return (
    <div
      {...rest}
      style={{
        background: "var(--panel)",
        border: "1px solid var(--border)",
        borderRadius: "var(--r-lg)",
        boxShadow: "var(--shadow-1)",
        overflow: "hidden",
        ...style,
      }}
    >
      {(title || actions) && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: "var(--sp-3)",
            padding: "var(--sp-3) var(--sp-4)",
            borderBottom: "1px solid var(--border)",
          }}
        >
          <div style={{ fontSize: "var(--fs-md)", fontWeight: 600, color: "var(--ink)" }}>
            {title}
          </div>
          {actions && <div style={{ display: "flex", gap: "var(--sp-2)" }}>{actions}</div>}
        </div>
      )}
      <div style={{ padding: padded ? "var(--sp-4)" : 0 }}>{children}</div>
    </div>
  );
}

// ---- Stat (metric tile) ---------------------------------------------------------------------

export function Stat({
  label,
  value,
  unit,
  tone = "ink",
  delta,
  spark,
  "data-testid": testId,
}: {
  label: ReactNode;
  value: ReactNode;
  unit?: string;
  tone?: Tone;
  delta?: { value: string; tone?: Tone };
  spark?: number[];
  "data-testid"?: string;
}) {
  return (
    <div
      data-testid={testId}
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "var(--sp-1)",
        padding: "var(--sp-3)",
        background: "var(--panel)",
        border: "1px solid var(--border)",
        borderRadius: "var(--r-md)",
        minWidth: 96,
      }}
    >
      <div
        style={{
          fontSize: "var(--fs-xs)",
          textTransform: "uppercase",
          letterSpacing: 0.5,
          color: "var(--muted)",
        }}
      >
        {label}
      </div>
      <div style={{ display: "flex", alignItems: "baseline", gap: "var(--sp-2)" }}>
        <span
          style={{
            fontFamily: "var(--font-mono)",
            fontSize: "var(--fs-2xl)",
            fontVariantNumeric: "tabular-nums",
            lineHeight: 1,
            color: TONE_FG[tone],
          }}
        >
          {value}
        </span>
        {unit && <span style={{ fontSize: "var(--fs-sm)", color: "var(--muted)" }}>{unit}</span>}
        {delta && (
          <span style={{ fontSize: "var(--fs-sm)", color: TONE_FG[delta.tone ?? "muted"] }}>
            {delta.value}
          </span>
        )}
      </div>
      {spark && spark.length > 1 && <Sparkline points={spark} tone={tone === "ink" ? "accent" : tone} />}
    </div>
  );
}

// ---- Sparkline (inline SVG) -----------------------------------------------------------------

export function Sparkline({
  points,
  tone = "accent",
  width = 52,
  height = 16,
}: {
  points: number[];
  tone?: Tone;
  width?: number;
  height?: number;
}) {
  if (points.length < 2) return null;
  const min = Math.min(...points);
  const max = Math.max(...points);
  const span = max - min || 1;
  const step = width / (points.length - 1);
  const d = points
    .map((p, i) => `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${(height - ((p - min) / span) * height).toFixed(1)}`)
    .join(" ");
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
      <path d={d} fill="none" stroke={TONE_FG[tone]} strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

// ---- Badge ----------------------------------------------------------------------------------

export function Badge({
  tone,
  status,
  children,
  "data-testid": testId,
}: {
  tone?: Tone;
  status?: string;
  children: ReactNode;
  "data-testid"?: string;
}) {
  const t: Tone = tone ?? (status ? statusTone(status) : "muted");
  return (
    <span
      data-testid={testId}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: "var(--sp-1)",
        padding: "1px var(--sp-2)",
        fontSize: "var(--fs-xs)",
        fontWeight: 600,
        borderRadius: 999,
        color: TONE_FG[t],
        background: TONE_WEAK_BG[t],
      }}
    >
      {children}
    </span>
  );
}

// ---- Button ---------------------------------------------------------------------------------

export function Button({
  variant = "primary",
  size = "md",
  loading = false,
  children,
  style,
  disabled,
  ...rest
}: {
  variant?: "primary" | "ghost" | "danger";
  size?: "md" | "sm";
  loading?: boolean;
} & ButtonHTMLAttributes<HTMLButtonElement>) {
  const base: CSSProperties = {
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "var(--sp-2)",
    padding: size === "sm" ? "var(--sp-1) var(--sp-2)" : "var(--sp-2) var(--sp-3)",
    fontSize: size === "sm" ? "var(--fs-sm)" : "var(--fs-md)",
    fontFamily: "var(--font-ui)",
    fontWeight: 600,
    borderRadius: "var(--r-md)",
    cursor: disabled || loading ? "not-allowed" : "pointer",
    opacity: disabled || loading ? 0.55 : 1,
    border: "1px solid transparent",
  };
  const variants: Record<string, CSSProperties> = {
    primary: { background: "var(--accent)", color: "var(--on-accent)", borderColor: "var(--accent)" },
    ghost: { background: "transparent", color: "var(--ink)", borderColor: "var(--border-strong)" },
    danger: { background: "var(--danger-weak)", color: "var(--danger)", borderColor: "transparent" },
  };
  return (
    <button {...rest} disabled={disabled || loading} style={{ ...base, ...variants[variant], ...style }}>
      {loading ? "…" : children}
    </button>
  );
}

// ---- Field / Input / TextArea / Select ------------------------------------------------------

export function Field({
  label,
  hint,
  error,
  children,
}: {
  label?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
}) {
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: "var(--sp-1)" }}>
      {label && <span style={{ fontSize: "var(--fs-sm)", color: "var(--muted)" }}>{label}</span>}
      {children}
      {error ? (
        <span role="alert" style={{ fontSize: "var(--fs-xs)", color: "var(--danger)" }}>
          {error}
        </span>
      ) : (
        hint && <span style={{ fontSize: "var(--fs-xs)", color: "var(--muted)" }}>{hint}</span>
      )}
    </label>
  );
}

const controlStyle: CSSProperties = {
  padding: "var(--sp-2) var(--sp-3)",
  fontSize: "var(--fs-md)",
  fontFamily: "var(--font-ui)",
  color: "var(--ink)",
  background: "var(--panel)",
  border: "1px solid var(--border-strong)",
  borderRadius: "var(--r-md)",
};

export function Input(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} style={{ ...controlStyle, ...props.style }} />;
}
export function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} style={{ ...controlStyle, resize: "vertical", ...props.style }} />;
}
export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} style={{ ...controlStyle, ...props.style }} />;
}

// ---- EmptyState -----------------------------------------------------------------------------

export function EmptyState({
  title,
  hint,
  action,
  "data-testid": testId,
}: {
  title: ReactNode;
  hint?: ReactNode;
  action?: ReactNode;
  "data-testid"?: string;
}) {
  return (
    <div
      data-testid={testId}
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: "var(--sp-2)",
        padding: "var(--sp-6) var(--sp-4)",
        textAlign: "center",
        color: "var(--muted)",
      }}
    >
      <div style={{ fontSize: "var(--fs-md)", color: "var(--ink)" }}>{title}</div>
      {hint && <div style={{ fontSize: "var(--fs-sm)", maxWidth: 320, lineHeight: 1.5 }}>{hint}</div>}
      {action}
    </div>
  );
}

// ---- Dialog (modal shell) -------------------------------------------------------------------

export function Dialog({
  open,
  title,
  onClose,
  footer,
  children,
  "data-testid": testId,
}: {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  footer?: ReactNode;
  children: ReactNode;
  "data-testid"?: string;
}) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      data-testid={testId}
      role="dialog"
      aria-modal="true"
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.4)",
        zIndex: 100,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        style={{
          width: "min(560px, calc(100vw - 2 * var(--sp-5)))",
          maxHeight: "calc(100vh - 2 * var(--sp-5))",
          overflow: "auto",
          background: "var(--panel)",
          border: "1px solid var(--border)",
          borderRadius: "var(--r-lg)",
          boxShadow: "var(--shadow-1)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "var(--sp-3) var(--sp-4)",
            borderBottom: "1px solid var(--border)",
          }}
        >
          <div style={{ fontSize: "var(--fs-lg)", fontWeight: 600, color: "var(--ink)" }}>{title}</div>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--muted)",
              cursor: "pointer",
              fontSize: "var(--fs-lg)",
            }}
          >
            ✕
          </button>
        </div>
        <div style={{ padding: "var(--sp-4)" }}>{children}</div>
        {footer && (
          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              gap: "var(--sp-2)",
              padding: "var(--sp-3) var(--sp-4)",
              borderTop: "1px solid var(--border)",
            }}
          >
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}
