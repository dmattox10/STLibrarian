/**
 * Electron backend.
 *
 * The renderer never touches Node directly -- it talks to a `contextBridge`
 * object the preload script exposes as `window.stlibrarian`. That bridge is the
 * whole contract, and it is deliberately the same set of verbs the Tauri
 * adapter implements:
 *
 *   window.stlibrarian = {
 *     listModels(query), getModel(id), listTags(), stats(),
 *     scan(path), setTags(modelId, tags), deleteModel(modelId),
 *     getThumb(hash), putThumb(hash, dataUrl),
 *     collections(), createCollection(name), renameCollection(id, name),
 *     deleteCollection(id),
 *     addToCollection(collectionId, modelIds),
 *     removeFromCollection(collectionId, modelIds),
 *     bulkAddTag(modelIds, tag), bulkRemoveTag(modelIds, tag),
 *     packages(), packageSuggestions(), createPackage(name, root, modelIds),
 *     renamePackage(id, name), deletePackage(id),
 *     assignToPackage(packageId, modelIds), unassignFromPackage(modelIds),
 *     duplicates(), creators(), setProvenance(modelId, creator, source),
 *     modelRenders(modelId), setModelRender(modelId, imageId),
 *     prints(modelId), logPrint(modelId, entry), deletePrint(printId),
 *     openInSlicer(modelId), revealInFolder(modelId),
 *     readModel(id) -> ArrayBuffer,
 *     readPreview(id, maxTriangles) -> ArrayBuffer,  // decimated positions
 *     pickFolder() -> string | null,
 *     onScanProgress(cb) -> unsubscribe,
 *     onFileDrop(cb) -> unsubscribe,   // cb({state:"over"|"drop"|"leave", paths})
 *     pruneThumbs(), missingFiles(),
 *   }
 *
 * Everything above `src/data/` is untouched by which shell is running, so
 * swapping Tauri for Electron is this file plus a main process -- not a
 * rewrite of the app.
 */
export function createElectronAdapter(bridge = globalThis.window?.stlibrarian) {
  if (!bridge) {
    throw new Error("Electron preload bridge (window.stlibrarian) is not available");
  }

  return {
    mode: "electron",
    needsAuth: false,

    async login() {
      return { token: null, user: { id: 0, username: "local", role: "admin" } };
    },
    async logout() {},
    async me() {
      return { id: 0, username: "local", role: "admin" };
    },

    listModels: (query = {}) => bridge.listModels(query),
    getModel: (id) => bridge.getModel(id),
    listTags: () => bridge.listTags(),
    stats: () => bridge.stats(),
    scan: (path) => bridge.scan(path),
    setTags: (modelId, tags) => bridge.setTags(modelId, tags),
    deleteModel: (modelId) => bridge.deleteModel(modelId),
    pruneThumbs: () => bridge.pruneThumbs?.() ?? 0,
    missingFiles: () => bridge.missingFiles?.() ?? [],

    collections: () => bridge.collections(),
    createCollection: (name) => bridge.createCollection(name),
    renameCollection: (id, name) => bridge.renameCollection(id, name),
    deleteCollection: (id) => bridge.deleteCollection(id),
    addToCollection: (collectionId, modelIds) => bridge.addToCollection(collectionId, modelIds),
    removeFromCollection: (collectionId, modelIds) =>
      bridge.removeFromCollection(collectionId, modelIds),

    bulkAddTag: (modelIds, tag) => bridge.bulkAddTag(modelIds, tag),

    packages: () => bridge.packages(),
    packageSuggestions: () => bridge.packageSuggestions(),
    createPackage: (name, root, modelIds) => bridge.createPackage(name, root, modelIds),
    renamePackage: (id, name) => bridge.renamePackage(id, name),
    deletePackage: (id) => bridge.deletePackage(id),
    assignToPackage: (packageId, modelIds) => bridge.assignToPackage(packageId, modelIds),
    unassignFromPackage: (modelIds) => bridge.unassignFromPackage(modelIds),

    duplicates: () => bridge.duplicates(),
    creators: () => bridge.creators(),
    setProvenance: (modelId, creator, source) =>
      bridge.setProvenance(modelId, creator, source),

    modelRenders: (modelId) => bridge.modelRenders(modelId),
    setModelRender: (modelId, imageId) => bridge.setModelRender(modelId, imageId),

    openInSlicer: bridge.openInSlicer ? (id) => bridge.openInSlicer(id) : null,
    revealInFolder: bridge.revealInFolder ? (id) => bridge.revealInFolder(id) : null,

    prints: (modelId) => bridge.prints(modelId),
    logPrint: (modelId, entry) => bridge.logPrint(modelId, entry),
    deletePrint: (printId) => bridge.deletePrint(printId),
    bulkRemoveTag: (modelIds, tag) => bridge.bulkRemoveTag(modelIds, tag),

    getThumbUrl: (hash) => bridge.getThumb(hash),
    putThumb: (hash, dataUrl) => bridge.putThumb(hash, dataUrl),
    readModel: (id) => bridge.readModel(id),
    readPreview: (id, maxTriangles) => bridge.readPreview(id, maxTriangles),
    pickFolder: () => bridge.pickFolder(),
    onScanProgress: (cb) => bridge.onScanProgress?.(cb) ?? (() => {}),
    onFileDrop: (cb) => bridge.onFileDrop?.(cb) ?? (() => {}),
  };
}
