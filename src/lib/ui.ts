import { api } from "./api";
import { setSettings, settings } from "./store";

export const sidebarCollapsed = () => settings()?.sidebarCollapsed ?? false;

/** NAV-10: collapsible sidebar; the state is saved in the settings. */
export async function toggleSidebar() {
  const s = settings();
  if (!s) return;
  setSettings({ ...s, sidebarCollapsed: !s.sidebarCollapsed });
  setSettings(await api.updateSettings({ sidebarCollapsed: !s.sidebarCollapsed }));
}

export function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
