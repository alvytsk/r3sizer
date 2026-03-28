import path from "path"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import wasm from "vite-plugin-wasm"
import tailwindcss from "@tailwindcss/vite"

export default defineConfig({
  base: "/r3sizer/",
  plugins: [react(), wasm(), tailwindcss()],
  worker: {
    plugins: () => [wasm()],
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    target: "esnext",
  },
})
