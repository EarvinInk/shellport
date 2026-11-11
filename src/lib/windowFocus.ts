import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Run `cb` whenever the app window gains focus. WebKitGTK doesn't reliably
 * fire the DOM `focus` event when the native window is re-focused, so this
 * listens to Tauri's window event too (DOM event as a fallback).
 */
export function onWindowFocus(cb: () => void): () => void {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  getCurrentWindow()
    .onFocusChanged(({ payload: focused }) => focused && cb())
    .then((u) => (cancelled ? u() : (unlisten = u)))
    .catch(() => {});
  window.addEventListener("focus", cb);
  return () => {
    cancelled = true;
    unlisten?.();
    window.removeEventListener("focus", cb);
  };
}
