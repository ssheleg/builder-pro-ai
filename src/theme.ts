/** Single minimal dark theme (spec §1 goal: light/minimal panel). Colors consumed by
 * components (App shell chrome + StatusDot's four lifecycle states). */
export interface Theme {
  colors: {
    bg: string;
    bgElevated: string;
    border: string;
    text: string;
    textDim: string;
    accent: string;
    statusIdle: string; // atPrompt / typing
    statusRunning: string; // running (no input wait)
    statusExited: string; // exited
    statusWaiting: string; // running + waitingForInput
  };
}

export const theme: Theme = {
  colors: {
    bg: "#0d1117",
    bgElevated: "#161b22",
    border: "#30363d",
    text: "#e6edf3",
    textDim: "#8b949e",
    accent: "#2f81f7",
    statusIdle: "#8b949e", // grey — idle at prompt
    statusRunning: "#2ea043", // green — command running
    statusExited: "#f85149", // red — process exited
    statusWaiting: "#d29922", // amber — waiting for input
  },
};
