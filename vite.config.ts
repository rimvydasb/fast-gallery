import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    target: "safari15",
    minify: "esbuild",
    cssMinify: true,
    modulePreload: false,
    reportCompressedSize: false,
  },
});
