import { render } from "solid-js/web";
import "./styles.css";
import { App } from "./App";
import { SettingsView } from "./views/SettingsView";

const root = document.getElementById("root")!;
render(() => (location.hash === "#settings" ? <SettingsView /> : <App />), root);

// Test hook for debug builds (vite build --mode development): exposes the window state.
if (import.meta.env.MODE === "development") {
  import("./lib/store").then((store) => ((window as unknown as { __fg: unknown }).__fg = store));
}
