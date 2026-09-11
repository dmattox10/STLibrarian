const TOKEN_KEY = "stlibrarian.token";

/**
 * Pro backend: the axum server over HTTP.
 *
 * Also the backend the browser uses when STLibrarian is installed on a NAS, so
 * this is the only adapter that deals in sessions.
 */
export function createHttpAdapter(baseUrl = "") {
  let token = null;
  try {
    token = localStorage.getItem(TOKEN_KEY);
  } catch {
    // Private browsing, or storage disabled. A session that lasts one page
    // load is still a working session.
  }

  const setToken = (value) => {
    token = value;
    try {
      if (value) localStorage.setItem(TOKEN_KEY, value);
      else localStorage.removeItem(TOKEN_KEY);
    } catch {
      /* see above */
    }
  };

  async function request(path, { method = "GET", body, raw = false } = {}) {
    const headers = {};
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (token) headers.Authorization = `Bearer ${token}`;

    const res = await fetch(`${baseUrl}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });

    if (res.status === 401) {
      // The session died server-side; drop it so the UI falls back to login
      // instead of retrying forever with a token that will never work.
      setToken(null);
      throw new AuthError("Your session has expired. Please sign in again.");
    }
    if (!res.ok) {
      let message = `${res.status} ${res.statusText}`;
      try {
        const payload = await res.json();
        if (payload?.error) message = payload.error;
      } catch {
        /* not a JSON error body */
      }
      throw new Error(message);
    }
    if (raw) return res.arrayBuffer();
    if (res.status === 204) return null;
    return res.json();
  }

  const qs = (query) => {
    const params = new URLSearchParams();
    if (query.search) params.set("search", query.search);
    if (query.tags?.length) params.set("tags", query.tags.join(","));
    if (query.collection != null) params.set("collection", String(query.collection));
    if (query.package != null) params.set("package", String(query.package));
    if (query.creator) params.set("creator", query.creator);
    if (query.printed != null) params.set("printed", String(query.printed));
    if (query.variant) params.set("variant", query.variant);
    if (query.sort) params.set("sort", query.sort);
    if (query.limit != null) params.set("limit", String(query.limit));
    if (query.offset != null) params.set("offset", String(query.offset));
    const s = params.toString();
    return s ? `?${s}` : "";
  };

  return {
    mode: "http",
    needsAuth: true,
    get token() {
      return token;
    },

    async login(username, password) {
      const result = await request("/api/login", {
        method: "POST",
        body: { username, password },
      });
      setToken(result.token);
      return result;
    },
    async logout() {
      try {
        await request("/api/logout", { method: "POST" });
      } finally {
        setToken(null);
      }
    },
    async me() {
      if (!token) return null;
      try {
        return await request("/api/me");
      } catch {
        return null;
      }
    },

    listModels: (query = {}) => request(`/api/models${qs(query)}`),
    getModel: (id) => request(`/api/models/${id}`),
    listTags: () => request("/api/tags"),
    stats: () => request("/api/stats"),
    scan: (path) => request("/api/scan", { method: "POST", body: { path } }),
    setTags: (modelId, tags) =>
      request(`/api/models/${modelId}/tags`, { method: "POST", body: { tags } }),
    deleteModel: (modelId) => request(`/api/models/${modelId}`, { method: "DELETE" }),
    async pruneThumbs() {
      return 0;
    },

    collections: () => request("/api/collections"),
    createCollection: (name, shared = true) =>
      request("/api/collections", { method: "POST", body: { name, shared } }),
    renameCollection: (id, name) =>
      request(`/api/collections/${id}`, { method: "PATCH", body: { name } }),
    deleteCollection: (id) => request(`/api/collections/${id}`, { method: "DELETE" }),
    addToCollection: (collectionId, modelIds) =>
      request(`/api/collections/${collectionId}/models`, {
        method: "POST",
        body: { model_ids: modelIds },
      }),
    removeFromCollection: (collectionId, modelIds) =>
      request(`/api/collections/${collectionId}/models/remove`, {
        method: "POST",
        body: { model_ids: modelIds },
      }),

    bulkAddTag: (modelIds, tag) =>
      request("/api/bulk/tags/add", { method: "POST", body: { model_ids: modelIds, tag } }),

    packages: () => request("/api/packages"),
    packageSuggestions: () => request("/api/packages/suggestions"),
    createPackage: (name, root, modelIds) =>
      request("/api/packages", { method: "POST", body: { name, root, model_ids: modelIds } }),
    renamePackage: (id, name) =>
      request(`/api/packages/${id}`, { method: "PATCH", body: { name } }),
    deletePackage: (id) => request(`/api/packages/${id}`, { method: "DELETE" }),
    assignToPackage: (packageId, modelIds) =>
      request(`/api/packages/${packageId}/models`, {
        method: "POST",
        body: { model_ids: modelIds },
      }),
    unassignFromPackage: (modelIds) =>
      request("/api/bulk/packages/unassign", { method: "POST", body: { model_ids: modelIds } }),

    duplicates: () => request("/api/duplicates"),
    creators: () => request("/api/creators"),
    setProvenance: (modelId, creator, source) =>
      request(`/api/models/${modelId}/provenance`, { method: "POST", body: { creator, source } }),

    modelRenders: (modelId) => request(`/api/models/${modelId}/renders`),
    setModelRender: (modelId, imageId) =>
      request(`/api/models/${modelId}/render`, { method: "POST", body: { image_id: imageId } }),

    // The files live on the server, not on whatever machine is browsing, so
    // there is nothing here for a slicer to open.
    users: () => request("/api/users"),
    createUser: (username, password, role) =>
      request("/api/users", { method: "POST", body: { username, password, role } }),
    setUserRole: (id, role) =>
      request(`/api/users/${id}`, { method: "PATCH", body: { role } }),
    setUserPassword: (id, password) =>
      request(`/api/users/${id}/password`, { method: "POST", body: { password } }),
    deleteUser: (id) => request(`/api/users/${id}`, { method: "DELETE" }),

    slices: (modelId) => request(`/api/models/${modelId}/slices`),
    orphanSlices: () => request("/api/slices/unattached"),
    setSliceOwner: (sliceId, modelId, packageId = null) =>
      request(`/api/slices/${sliceId}/owner`, {
        method: "POST",
        body: { model_id: modelId, package_id: packageId },
      }),
    // A browser cannot hand a file to a local slicer, but it can download one.
    openSlice: null,
    sliceFileUrl: (sliceId) => `${baseUrl}/api/slices/${sliceId}/file`,

    // Pro only: the files are on the server, so the server is the only thing
    // that can move them -- which is also why this exists here and nowhere else.
    organizePlan: (root) =>
      request("/api/organize/plan", { method: "POST", body: { root } }),
    organizeApply: (root) =>
      request("/api/organize/apply", { method: "POST", body: { root } }),

    openInSlicer: null,
    revealInFolder: null,

    prints: (modelId) => request(`/api/models/${modelId}/prints`),
    logPrint: (modelId, entry) =>
      request(`/api/models/${modelId}/prints`, { method: "POST", body: entry }),
    deletePrint: (printId) => request(`/api/prints/${printId}`, { method: "DELETE" }),
    bulkRemoveTag: (modelIds, tag) =>
      request("/api/bulk/tags/remove", { method: "POST", body: { model_ids: modelIds, tag } }),
    async missingFiles() {
      return [];
    },

    // The server sets a long immutable cache header, and the URL is keyed by
    // content hash, so the browser only ever fetches each preview once.
    async getThumbUrl(hash) {
      // Always a URL: the server rasterises on first request and caches by
      // content hash, so there is nothing to wait for it to have already done.
      return `${baseUrl}/api/thumb/${hash}`;
    },
    putThumb: (hash, dataUrl) =>
      request(`/api/thumb/${hash}`, { method: "PUT", body: { data_url: dataUrl } }),

    readModel: (id) => request(`/api/models/${id}/file`, { raw: true }),
    readPreview: (id, maxTriangles) =>
      request(`/api/models/${id}/preview?max=${maxTriangles}`, { raw: true }),

    // A browser cannot open a native folder picker, so Pro takes a typed path.
    // It is the server's filesystem being scanned, not the viewer's.
    async pickFolder() {
      return null;
    },

    // A browser drop yields File objects, never a path on the server's disk,
    // and the server is what does the scanning. Declining is honest; the path
    // field in the rail is the way in for Pro.
    onFileDrop() {
      return () => {};
    },

    onScanProgress() {
      return () => {};
    },
  };
}

export class AuthError extends Error {}
