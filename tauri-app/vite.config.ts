import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri drives the dev server on a fixed port and needs a stable target so the
// Rust process can reach it. `TAURI_DEV_HOST` is set by the CLI for device
// testing; leaving it unset keeps the local flow simple.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      // Rust sources are watched by the Tauri CLI, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // WKWebView on macOS 14 and WebViewGTK2 both ship ES2021.
    target: "es2021",
    // Vite 8 bundles with rolldown, so the minifier is oxc; the esbuild
    // minifier of earlier lines is no longer installed.
    minify: process.env.TAURI_ENV_DEBUG ? false : "oxc",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    outDir: "dist",
    emptyOutDir: true,
  },
});
