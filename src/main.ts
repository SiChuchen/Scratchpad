import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Component } from "svelte";
import App from "./App.svelte";
import MinimizedApp from "./MinimizedApp.svelte";
import QuickAccessApp from "./QuickAccessApp.svelte";
import "./app.css";

// Disable default browser context menu in WebView
document.addEventListener("contextmenu", (e) => e.preventDefault());

const label = getCurrentWindow().label;

// Route by window label — each Tauri window gets its own root component.
const roots: Record<string, Component> = {
  main: App,
  "minimized-tab": MinimizedApp,
  "quick-access": QuickAccessApp,
};

const Root: Component = roots[label] ?? App;

// Apply label-specific class for app.css scoping.
const labelClass =
  label === "minimized-tab"
    ? "minimized-tab-window"
    : label === "quick-access"
      ? "quick-access-window"
      : "main-window";
document.documentElement.classList.add(labelClass);

const app = mount(Root, {
  target: document.getElementById("app")!,
});

export default app;
