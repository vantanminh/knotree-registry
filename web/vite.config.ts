import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 4173,
    allowedHosts: ["registry.knotree.com"],
    proxy: {
      "/api": "http://127.0.0.1:8080",
      "/auth": "http://127.0.0.1:8080",
      "/v2": "http://127.0.0.1:8080",
      "/livez": "http://127.0.0.1:8080",
      "/readyz": "http://127.0.0.1:8080",
      "/health": "http://127.0.0.1:8080"
    }
  }
});
