import { defineConfig } from "vitest/config";

// The unit tests cover the pure helpers, so they run in a plain Node
// environment with no webview and no Tauri runtime.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
