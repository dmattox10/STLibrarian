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
    async missingFiles() {
      return [];
    },

    // The server sets a long immutable cache header, and the URL is keyed by
    // content hash, so the browser only ever fetches each preview once.
    async getThumbUrl(hash, model) {
      return model?.has_thumb ? `${baseUrl}/api/thumb/${hash}` : null;
    },
    putThumb: (hash, dataUrl) =>
      request(`/api/thumb/${hash}`, { method: "PUT", body: { data_url: dataUrl } }),

    readModel: (id) => request(`/api/models/${id}/file`, { raw: true }),

    // A browser cannot open a native folder picker, so Pro takes a typed path.
    // It is the server's filesystem being scanned, not the viewer's.
    async pickFolder() {
      return null;
    },

    onScanProgress() {
      return () => {};
    },
  };
}

export class AuthError extends Error {}
