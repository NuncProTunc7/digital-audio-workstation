/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri expects a fixed dev port and must not have Vite clear its output.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    // The sheet music renderer (~1.4 MB) is its own chunk, loaded only when
    // the Sheet music tab opens; a desktop app reads it from disk.
    chunkSizeWarningLimit: 2000,
  },
  test: {
    environment: "jsdom",
  },
});
