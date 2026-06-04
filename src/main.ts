import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { injectTheme } from "./lib/theme-loader";

// Inject default theme CSS synchronously before mount so CSS variables are defined immediately
injectTheme("");

mount(App, { target: document.getElementById("app")! });
