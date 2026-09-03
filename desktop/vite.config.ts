import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const uiSrc = fileURLToPath(new URL("../ui/src", import.meta.url));

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5174, strictPort: true },
  resolve: {
    alias: {
      "@ferry/ui/lib": `${uiSrc}/lib`,
      "@ferry/ui/styles": `${uiSrc}/styles`,
      "@ferry/ui/App": `${uiSrc}/App.tsx`,
      "@": uiSrc,
      "@bindings": fileURLToPath(new URL("../bindings", import.meta.url)),
    },
  },
});
