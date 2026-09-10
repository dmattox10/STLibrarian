import { createTauriAdapter } from "./tauri.js";
import { createElectronAdapter } from "./electron.js";
import { createHttpAdapter } from "./http.js";

/**
 * Pick a backend from whatever the page is running inside.
 *
 * The UI imports `api` and never learns which one it got. That is the whole
 * point: the library grid, the Nord controls and the STL viewer are the bulk of
 * the work and none of it cares whether Rust, Node or an HTTP server is
 * answering.
 */
function detect() {
  if (typeof window === "undefined") return "http";
  if (window.__TAURI_INTERNALS__ || window.__TAURI__) return "tauri";
  if (window.stlibrarian) return "electron";
  return "http";
}

function build() {
  switch (detect()) {
    case "tauri":
      return createTauriAdapter();
    case "electron":
      return createElectronAdapter();
    default:
      // Served by the Pro server itself, so same-origin.
      return createHttpAdapter("");
  }
}

export const api = build();
export { AuthError } from "./http.js";
