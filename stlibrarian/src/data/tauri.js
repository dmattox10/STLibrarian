import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
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

    collections: () => invoke("collections"),
    createCollection: (name) => invoke("create_collection", { name }),
    renameCollection: (id, name) => invoke("rename_collection", { id, name }),
    deleteCollection: (id) => invoke("delete_collection", { id }),
    addToCollection: (collectionId, modelIds) =>
      invoke("add_to_collection", { collectionId, modelIds }),
    removeFromCollection: (collectionId, modelIds) =>
      invoke("remove_from_collection", { collectionId, modelIds }),

    bulkAddTag: (modelIds, tag) => invoke("bulk_add_tag", { modelIds, tag }),

    packages: () => invoke("packages"),
    packageSuggestions: () => invoke("package_suggestions"),
    createPackage: (name, root, modelIds) =>
      invoke("create_package", { name, root, modelIds }),
    renamePackage: (id, name) => invoke("rename_package", { id, name }),
    deletePackage: (id) => invoke("delete_package", { id }),
    assignToPackage: (packageId, modelIds) =>
      invoke("assign_to_package", { packageId, modelIds }),
    unassignFromPackage: (modelIds) => invoke("unassign_from_package", { modelIds }),

    duplicates: () => invoke("duplicates"),
    creators: () => invoke("creators"),
    setProvenance: (modelId, creator, source) =>
      invoke("set_provenance", { modelId, creator, source }),

    modelRenders: (modelId) => invoke("model_renders", { modelId }),
    setModelRender: (modelId, imageId) => invoke("set_model_render", { modelId, imageId }),

    openInSlicer: (modelId) => invoke("open_in_slicer", { modelId }),
    revealInFolder: (modelId) => invoke("reveal_in_folder", { modelId }),

    // The desktop app has one person and no accounts.
    users: null,

    slices: (modelId) => invoke("slices", { modelId }),
    orphanSlices: () => invoke("orphan_slices"),
    setSliceOwner: (sliceId, modelId, packageId = null) =>
      invoke("set_slice_owner", { sliceId, modelId, packageId }),
    // The slice object is only needed by the remote adapter, which has to
    // name a download; locally the id is enough.
    openSlice: (sliceId) => invoke("open_slice", { sliceId }),

    // Deliberately absent on the desktop: rearranging a collection is the
    // Pro server's job, so it is built once rather than per platform.
    organizePlan: null,
    organizeApply: null,

    prints: (modelId) => invoke("prints", { modelId }),
    logPrint: (modelId, entry) => invoke("log_print", { modelId, entry }),
    deletePrint: (printId) => invoke("delete_print", { printId }),
    bulkRemoveTag: (modelIds, tag) => invoke("bulk_remove_tag", { modelIds, tag }),
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

    /** Decimated mesh -- what the grid and viewer load. */
    async readPreview(id, maxTriangles) {
      const bytes = await invoke("preview_mesh", { id, maxTriangles });
      return bytes instanceof ArrayBuffer ? bytes : new Uint8Array(bytes).buffer;
    },

    async pickFolder() {
      const picked = await open({ directory: true, multiple: false });
      return typeof picked === "string" ? picked : null;
    },

    /**
     * Files and folders dropped onto the window.
     *
     * With dragDropEnabled the OS drop is captured by Tauri and delivered
     * here, which also means the webview never sees an HTML5 drop event --
     * this listener is the only way to know it happened.
     */
    onFileDrop(callback) {
      const pending = getCurrentWebview().onDragDropEvent((event) => {
        const kind = event.payload?.type;
        if (kind === "over" || kind === "enter" || kind === "dragOver") {
          callback({ state: "over" });
        } else if (kind === "drop" || kind === "dragDrop") {
          callback({ state: "drop", paths: event.payload.paths ?? [] });
        } else {
          callback({ state: "leave" });
        }
      });
      return () => pending.then((fn) => fn()).catch(() => {});
    },

    onScanProgress(callback) {
      const unlisten = listen("scan:progress", (event) => callback(event.payload));
      return () => unlisten.then((fn) => fn()).catch(() => {});
    },
  };
}
