import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import path from "node:path";

// https://tauri.app/v1/api/config
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: path.resolve(__dirname, "src/lib"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  // MOTODESK_CLOUD_URL is read by the Rust side too: one variable sets the cloud
  // address for the whole build.
  envPrefix: ["VITE_", "TAURI_ENV_*", "MOTODESK_CLOUD_URL"],
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
}));
