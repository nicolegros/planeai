import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks: {
          xterm: [
            "@xterm/xterm",
            "@xterm/addon-fit",
            "@xterm/addon-webgl",
            "@xterm/addon-web-links",
            "@xterm/addon-unicode11",
          ],
          codemirror: [
            "codemirror",
            "@codemirror/view",
            "@codemirror/state",
            "@codemirror/commands",
            "@codemirror/language",
            "@codemirror/search",
            "@codemirror/autocomplete",
            "@codemirror/language-data",
          ],
        },
      },
    },
  },
});
