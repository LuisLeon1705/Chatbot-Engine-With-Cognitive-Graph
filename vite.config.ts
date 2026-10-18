import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The frontend lives in ui/; Tauri serves it from :1420 in development and bundles dist/.
export default defineConfig({
  root: "ui",
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/worlds/**", "**/models/**"] },
  },
  build: {
    // A desktop app: the lazily loaded 3D view (three.js) is a large chunk on purpose.
    chunkSizeWarningLimit: 1600,
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
