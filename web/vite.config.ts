import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { licenseNotices } from "./license-notices.mjs";

export default defineConfig({
  plugins: [react(), licenseNotices(), {
    name: "cloudflare-script-compatibility",
    transformIndexHtml: {
      order: "post",
      // Vite replaces the entry script, so preserve this on generated tags too.
      handler: (html) => html.replace(/<script\b(?![^>]*\bdata-cfasync\b)/g, '<script data-cfasync="false"'),
    },
  }],
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
