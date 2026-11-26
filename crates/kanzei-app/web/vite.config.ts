import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";

const webRoot = path.dirname(fileURLToPath(import.meta.url));
const appRoot = path.resolve(webRoot, "..");

export default defineConfig({
  root: webRoot,
  base: "./",
  plugins: [react()],
  publicDir: path.resolve(webRoot, "public"),
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: path.resolve(appRoot, "dist-web"),
    emptyOutDir: true,
    target: "es2022",
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/react") || id.includes("node_modules/react-dom")) {
            return "vendor-react";
          }
          if (id.includes("node_modules/@tanstack")) {
            return "vendor-virtual";
          }
          return undefined;
        },
      },
    },
  },
});
