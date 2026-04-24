import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.svelte";
import MinimizedApp from "./MinimizedApp.svelte";
import "./app.css";

// Disable default browser context menu in WebView
document.addEventListener("contextmenu", (e) => e.preventDefault());

const label = getCurrentWindow().label;

const app = mount(label === "minimized-tab" ? MinimizedApp : App, {
  target: document.getElementById("app")!,
});

export default app;
