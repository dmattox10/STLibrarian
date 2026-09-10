import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./data/adapter.js";
import Sidebar from "./components/Sidebar.jsx";
import LibraryGrid from "./components/LibraryGrid.jsx";
import Inspector from "./components/Inspector.jsx";
import LoginScreen from "./components/LoginScreen.jsx";
import BulkBar from "./components/BulkBar.jsx";
import { useSelection } from "./lib/useSelection.js";
import { useThumbnails } from "./lib/useThumbnails.js";
import {
  Alert,
  Button,
  CubeIcon,
  Field,
  SearchField,
  Segmented,
} from "./components/controls/index.jsx";
import { formatCount } from "./lib/format.js";

const SORTS = [
  { value: "recent", label: "New" },
  { value: "name", label: "A-Z" },
  { value: "size", label: "Size" },
  { value: "triangles", label: "Detail" },
];

const EMPTY_STATS = { models: 0, total_bytes: 0, tags: 0, missing: 0, without_thumb: 0 };

export default function App() {
  // `checking` until we know whether a session exists, so the login screen
  // never flashes in front of an already-signed-in user.
  const [session, setSession] = useState(api.needsAuth ? { status: "checking" } : {
    status: "ready",
    user: { id: 0, username: "local", role: "admin" },
  });

  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [activeTags, setActiveTags] = useState([]);
  const [activeCollection, setActiveCollection] = useState(null);
  const [sort, setSort] = useState("recent");
  const [cardSize, setCardSize] = useState(190);

  const [models, setModels] = useState([]);
  const [tags, setTags] = useState([]);
  const [collections, setCollections] = useState([]);
  const [stats, setStats] = useState(EMPTY_STATS);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [selected, setSelected] = useState(null);
  const [reloadToken, setReloadToken] = useState(0);

  const requestId = useRef(0);

  useEffect(() => {
    if (!api.needsAuth) return;
    api
      .me()
      .then((user) =>
        setSession(user ? { status: "ready", user } : { status: "anon" })
      )
      .catch(() => setSession({ status: "anon" }));
  }, []);

  // Typing should not fire a query per keystroke against a NAS.
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 220);
    return () => clearTimeout(timer);
  }, [search]);

  const query = useMemo(
    () => ({
      search: debouncedSearch,
      tags: activeTags,
      collection: activeCollection,
      sort,
      limit: 500,
    }),
    [debouncedSearch, activeTags, activeCollection, sort]
  );

  const refresh = useCallback(() => setReloadToken((n) => n + 1), []);

  useEffect(() => {
    if (session.status !== "ready") return undefined;

    const mine = ++requestId.current;
    let cancelled = false;
    setLoading(true);

    Promise.all([api.listModels(query), api.listTags(), api.stats(), api.collections()])
      .then(([nextModels, nextTags, nextStats, nextCollections]) => {
        // Out-of-order responses must not overwrite a newer result.
        if (cancelled || requestId.current !== mine) return;
        setModels(nextModels);
        setTags(nextTags);
        setStats(nextStats);
        setCollections(nextCollections);
        setError(null);
      })
      .catch((e) => {
        if (cancelled || requestId.current !== mine) return;
        if (e?.name === "AuthError" || /session/i.test(e?.message || "")) {
          setSession({ status: "anon" });
          return;
        }
        setError(e?.message || "Could not load the library.");
      })
      .finally(() => {
        if (!cancelled && requestId.current === mine) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [query, reloadToken, session.status]);

  // Keep the inspector showing the current row after a tag edit or a rescan.
  useEffect(() => {
    if (!selected) return;
    const fresh = models.find((m) => m.id === selected.id);
    if (fresh && fresh !== selected) setSelected(fresh);
  }, [models, selected]);

  const thumbs = useThumbnails(api, models);
  const { marked, click, selectAll, clear } = useSelection(models, setSelected);

  // Escape is the universal "never mind" for a selection.
  useEffect(() => {
    const onKey = (event) => {
      if (event.key === "Escape") clear();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [clear]);

  const toggleTag = useCallback((name) => {
    setActiveTags((current) =>
      current.includes(name) ? current.filter((t) => t !== name) : [...current, name]
    );
  }, []);

  if (session.status === "checking") {
    return <div className="login"><span className="dim">Loading…</span></div>;
  }

  if (session.status === "anon") {
    return (
      <LoginScreen api={api} onSignedIn={(user) => setSession({ status: "ready", user })} />
    );
  }

  const isAdmin = session.user?.role === "admin";
  const filtered = activeTags.length > 0 || debouncedSearch || activeCollection != null;
  const markedIds = [...marked];

  return (
    <div className="app">
      <div className="brand">
        <span className="brand__mark">
          <CubeIcon />
        </span>
        <span className="brand__name">
          ST<em>Librarian</em>
        </span>
      </div>

      <header className="topbar">
        <SearchField
          value={search}
          onChange={setSearch}
          placeholder="Search models"
          style={{ width: 280 }}
        />
        <Segmented value={sort} onChange={setSort} options={SORTS} label="Sort order" />

        <div className="topbar__spacer" />

        <label className="row" style={{ gap: 7 }} title="Card size">
          <span className="visually-hidden">Card size</span>
          <input
            className="gc-slider"
            style={{ width: 96 }}
            type="range"
            min="140"
            max="300"
            step="10"
            value={cardSize}
            onChange={(e) => setCardSize(Number(e.target.value))}
          />
        </label>

        <span className="gc-badge">
          {formatCount(models.length)}
          {filtered ? ` of ${formatCount(stats.models)}` : ""}
        </span>

        {api.needsAuth ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              api.logout().finally(() => setSession({ status: "anon" }))
            }
          >
            Sign out
          </Button>
        ) : null}
      </header>

      <nav className="rail">
        <Sidebar
          api={api}
          tags={tags}
          activeTags={activeTags}
          onToggleTag={toggleTag}
          collections={collections}
          activeCollection={activeCollection}
          onSelectCollection={setActiveCollection}
          stats={stats}
          onRefresh={refresh}
          canScan={isAdmin}
        />
      </nav>

      <main className="main">
        <div className="stage">
          {error ? (
            <div style={{ marginBottom: 12 }}>
              <Alert>{error}</Alert>
            </div>
          ) : null}

          <LibraryGrid
            models={models}
            thumbs={thumbs}
            selected={selected}
            marked={marked}
            onSelect={click}
            cardSize={cardSize}
            loading={loading}
            emptyState={
              filtered
                ? {
                    title: "Nothing matches",
                    body: "No model in the library matches this search and tag combination.",
                    action: (
                      <Button
                        size="sm"
                        onClick={() => {
                          setSearch("");
                          setActiveTags([]);
                          setActiveCollection(null);
                        }}
                      >
                        Clear filters
                      </Button>
                    ),
                  }
                : {
                    title: "The library is empty",
                    body: isAdmin
                      ? "Point STLibrarian at a folder of STL files and it will index everything inside, including subfolders."
                      : "No models have been added to this server yet.",
                  }
            }
          />

          <BulkBar
            api={api}
            ids={markedIds}
            collections={collections}
            total={models.length}
            onDone={refresh}
            onClear={clear}
            onSelectAll={selectAll}
          />
        </div>

        {selected ? (
          <Inspector
            api={api}
            model={selected}
            onClose={() => setSelected(null)}
            onChanged={refresh}
            canDelete={isAdmin}
          />
        ) : null}
      </main>
    </div>
  );
}
