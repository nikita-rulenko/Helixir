import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    exclude: ["e2e/**", "node_modules/**", "dist/**"],
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
  server: {
    port: 4173,
    proxy: {
      "/api": "http://127.0.0.1:6971",
    },
  },
});
