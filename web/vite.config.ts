import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { licenseNotices } from "./license-notices.mjs";

export default defineConfig({
  plugins: [react(), licenseNotices()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:1990",
      "/health": "http://127.0.0.1:1990",
    },
  },
  build: {
    outDir: "dist",
    sourcemap: false,
    rollupOptions: { output: { manualChunks: { charts: ["recharts"] } } },
  },
});
