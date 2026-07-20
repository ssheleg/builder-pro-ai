import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "@fontsource/space-grotesk/500.css";
import "@fontsource/space-grotesk/700.css";
import "./ui/tokens.css";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { initTheme } from "./ui/theme";

// Apply the persisted light/dark/system theme BEFORE the first render so there is no flash of the
// wrong palette (S-UXR B1).
initTheme();

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("root element #root not found");

createRoot(rootEl).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
