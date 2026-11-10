import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { proxiedOrigin, REFINE_ORIGIN } from "./src/dev/proxyOrigin";

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "dist",
  },
  server: {
    proxy: {
      "/api": {
        target: REFINE_ORIGIN,
        // Rewrites Host only. Origin is rewritten below, for the dev page alone.
        changeOrigin: true,
        configure: (proxy) => {
          proxy.on("proxyReq", (proxyReq, req) => {
            const origin = req.headers.origin;
            const rewritten = proxiedOrigin(origin, req.headers.host);
            if (rewritten !== undefined && rewritten !== origin) {
              proxyReq.setHeader("origin", rewritten);
            }
          });
        },
      },
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["src/test-setup.ts"],
    exclude: ["e2e/**", "node_modules/**"],
  },
});
