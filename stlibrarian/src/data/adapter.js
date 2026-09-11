import { invoke } from "@tauri-apps/api/core";
import { createTauriAdapter } from "./tauri.js";
import { createElectronAdapter } from "./electron.js";
import { createHttpAdapter } from "./http.js";

const REMOTE_KEY = "stlibrarian.remote";

/** The remote library this desktop client is pointed at, if any. */
export function readRemote() {
  try {
    return localStorage.getItem(REMOTE_KEY) || null;
  } catch {
    return null;
  }
}

export function writeRemote(url) {
  try {
    if (url) localStorage.setItem(REMOTE_KEY, url.replace(/\/+$/, ""));
    else localStorage.removeItem(REMOTE_KEY);
  } catch {
    // Storage disabled. The setting will not survive a restart, which is
    // annoying rather than broken.
  }
}

export function isDesktop() {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__ || window.__TAURI__);
}

/**
 * A desktop client talking to somebody else's library.
 *
 * Data comes over HTTP like any browser, but the local capabilities come back:
 * a browser can offer you a download, it cannot put the file into Lychee on
 * this machine. That single difference is the whole case for running the
 * desktop app against a remote server, so it is what this adds.
 */
function createRemoteDesktopAdapter(baseUrl) {
  const http = createHttpAdapter(baseUrl);

  // Every method closes over the HTTP adapter's own state, so spreading is
  // safe -- nothing here reads `token` off the copied object.
  const fetchAndOpen = (url, filename) =>
    invoke("open_remote", { url, token: http.token, filename });

  return {
    ...http,
    mode: "remote",
    remote: baseUrl,

    async openInSlicer(modelId) {
      const model = await http.getModel(modelId);
      const filename = model.path.split("/").pop() || `${model.name}.stl`;
      return fetchAndOpen(`${baseUrl}/api/models/${modelId}/file`, filename);
    },

    async openSlice(sliceId, slice) {
      const filename = slice
        ? `${slice.name}.${slice.format}`
        : `slice-${sliceId}`;
      return fetchAndOpen(`${baseUrl}/api/slices/${sliceId}/file`, filename);
    },

    // There is nothing local to reveal: the file lives on the server, and what
    // this machine has is a cached copy it should not be taught to care about.
    revealInFolder: null,

    clearCache: () => invoke("clear_remote_cache"),
  };
}

/**
 * Pick a backend from where the page is running and what it has been told.
 *
 * The UI imports `api` and never learns which one it got. That is the point:
 * the grid, the Nord controls and the viewer are the bulk of the work and none
 * of it cares whether Rust, Node, or a server across the room is answering.
 */
function build() {
  if (isDesktop()) {
    const remote = readRemote();
    return remote ? createRemoteDesktopAdapter(remote) : createTauriAdapter();
  }
  if (typeof window !== "undefined" && window.stlibrarian) {
    return createElectronAdapter();
  }
  // Served by the Pro server itself, so same-origin.
  return createHttpAdapter("");
}

export const api = build();
export { AuthError } from "./http.js";
