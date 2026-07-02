import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/  (Tauri flavor: https://v2.tauri.app/start/frontend/vite/)
export default defineConfig(async () => ({
  plugins: [react()],
  // Tauri expects a fixed port and must fail (not silently fall back) if it is taken.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 5174 }
      : undefined,
    watch: {
      // Rust rebuilds must not trigger a frontend HMR reload.
      ignored: ["**/src-tauri/**"],
    },
  },
  // Only expose vars Tauri whitelists to the frontend.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Safari 13 is the macOS WKWebView floor Tauri targets.
    target: "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
}));
