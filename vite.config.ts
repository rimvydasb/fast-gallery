/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
    // Solid must load its browser build, so that signals and rendering work in jsdom.
    server: { deps: { inline: [/solid-js/] } },
  },
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    target: "safari15",
    minify: "esbuild",
    cssMinify: true,
    modulePreload: false,
    reportCompressedSize: false,
  },
});
