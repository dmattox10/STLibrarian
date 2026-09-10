import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

/**
 * Desktop backend: Rust over Tauri's IPC.
 *
 * The library is on this machine and there is exactly one user, so nothing here
 * authenticates. `needsAuth` is what the UI reads to decide whether to show a
 * login screen at all.
 */
export function createTauriAdapter() {
  return {
    mode: "tauri",
    needsAuth: false,

    async login() {
      return { token: null, user: { id: 0, username: "local", role: "admin" } };
    },
    async logout() {},
    async me() {
      return { id: 0, username: "local", role: "admin" };
    },

    listModels: (query = {}) => invoke("library", { query }),
    getModel: (id) => invoke("model", { id }),
    listTags: () => invoke("tags"),
    stats: () => invoke("stats"),
    scan: (path) => invoke("scan", { path }),
    setTags: (modelId, tags) => invoke("set_tags", { modelId, tags }),
    deleteModel: (modelId) => invoke("delete_model", { modelId }),
    pruneThumbs: () => invoke("prune_thumbs"),
    missingFiles: () => invoke("missing_files"),

    // Returns a data: URL, which drops straight into an <img src>.
    getThumbUrl: (hash) => invoke("get_thumb", { hash }),
    putThumb: (hash, dataUrl) => invoke("put_thumb", { hash, dataUrl }),

    // The command hands back raw bytes rather than a JSON number array; an STL
    // serialised as JSON would be several times its own size.
    async readModel(id) {
      const bytes = await invoke("read_model", { id });
      return bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer;
    },

    async pickFolder() {
      const picked = await open({ directory: true, multiple: false });
      return typeof picked === "string" ? picked : null;
    },

    onScanProgress(callback) {
      const unlisten = listen("scan:progress", (event) => callback(event.payload));
      return () => unlisten.then((fn) => fn()).catch(() => {});
    },
  };
}
