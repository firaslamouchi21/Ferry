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
  server: { port: 5173 },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
});
