import { mount } from "svelte";
import "@fontsource-variable/ibm-plex-sans";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "./app.css";
import { NO_THEME, injectTheme } from "./lib/theme-loader";

// Inject default theme CSS synchronously before mount so CSS variables are defined immediately
injectTheme(NO_THEME);

const page = new URLSearchParams(window.location.search).get("page");

if (page === "preferences") {
  import("./components/settings/SettingsWindow.svelte").then(({ default: SettingsWindow }) => {
    mount(SettingsWindow, { target: document.getElementById("app")! });
  });
} else {
  // Check if we're in benchmark replay mode
  import("@tauri-apps/api/core").then(({ invoke }) => {
    invoke("bench_get_config").then((config: unknown) => {
      if (config) {
        import("./components/BenchmarkRunner.svelte").then(({ default: BenchmarkRunner }) => {
          mount(BenchmarkRunner, {
            target: document.getElementById("app")!,
            props: { config: config as import("./lib/benchmark/replay").ReplayOptions },
          });
        });
      } else {
        import("./App.svelte").then(({ default: App }) => {
          mount(App, { target: document.getElementById("app")! });
        });
      }
    });
  });
}
