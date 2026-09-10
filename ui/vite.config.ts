import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { attachBridge } from "./dev-bridge/bridge.js";

const bridgePlugin = () => ({
  name: "ferry-daemon-bridge",
  apply: "serve" as const,
  configureServer(server: { httpServer: unknown; middlewares: unknown }) {
    if (server.httpServer) attachBridge(server);
  },
});

export default defineConfig({
  plugins: [react(), bridgePlugin()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      "@bindings": fileURLToPath(new URL("../bindings", import.meta.url)),
    },
  },
  server: {
    port: Number(process.env.PORT ?? 5173),
    host: true,
    strictPort: false,
    hmr:
      process.env.VITE_HMR_HOST || process.env.VITE_HMR_CLIENT_PORT
        ? {
            host: process.env.VITE_HMR_HOST,
            clientPort: process.env.VITE_HMR_CLIENT_PORT ? Number(process.env.VITE_HMR_CLIENT_PORT) : undefined,
            protocol: process.env.VITE_HMR_PROTOCOL,
          }
        : undefined,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});
