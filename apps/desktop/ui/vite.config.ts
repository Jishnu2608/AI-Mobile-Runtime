import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 27300, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true, target: "es2022" },
});
