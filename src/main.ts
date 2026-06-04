import { mount } from "svelte";
import "./app.css";
import { injectTheme } from "./lib/theme-loader";

// Inject default theme CSS synchronously before mount so CSS variables are defined immediately
injectTheme("");

const page = new URLSearchParams(window.location.search).get("page");

if (page === "preferences") {
  import("./components/PreferencesPage.svelte").then(({ default: PreferencesPage }) => {
    mount(PreferencesPage, { target: document.getElementById("app")! });
  });
} else {
  import("./App.svelte").then(({ default: App }) => {
    mount(App, { target: document.getElementById("app")! });
  });
}
