import { useEffect, useState } from "react";
import { Alert, Button, Field, Panel } from "./controls/index.jsx";
import { formatBytes, formatCount } from "../lib/format.js";

/**
 * Packages: several files that are one model.
 *
 * Suggestions are proposals, never applied on their own. A folder of STLs is
 * as often a folder of unrelated models as it is one model in pieces, so
 * nothing gets grouped until somebody says so.
 */
export default function PackagesPanel({
  api,
  packages,
  activePackage,
  onSelect,
  onChanged,
}) {
  const [suggestions, setSuggestions] = useState([]);
  const [busy, setBusy] = useState(null);
  const [renaming, setRenaming] = useState(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState(null);
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .packageSuggestions()
      .then((s) => !cancelled && setSuggestions(s))
      .catch(() => !cancelled && setSuggestions([]));
    return () => {
      cancelled = true;
    };
  }, [api, packages]);

  const guard = async (action) => {
    setError(null);
    try {
      await action();
      onChanged();
    } catch (e) {
      setError(e?.message || "That did not work.");
    }
  };

  const combine = (s) =>
    guard(async () => {
      setBusy(s.root);
      try {
        await api.createPackage(s.name, s.root, s.model_ids);
      } finally {
        setBusy(null);
      }
    });

  const visible = showAll ? suggestions : suggestions.slice(0, 4);

  // Two folders often reduce to the same model name -- a duplicate copy, or
  // supported and unsupported side by side. Identical rows with identical
  // buttons are unusable, so collisions get the folder as a subtitle.
  const nameCounts = suggestions.reduce((acc, s) => {
    acc[s.name] = (acc[s.name] || 0) + 1;
    return acc;
  }, {});
  const folderOf = (root) => root.split("/").filter(Boolean).pop() || root;

  return (
    <Panel title="Packages">
      <div className="stack">
        {packages.length === 0 && suggestions.length === 0 ? (
          <span className="dim" style={{ fontSize: 12, lineHeight: 1.5 }}>
            When several files are one model, group them here. Nothing is
            grouped automatically.
          </span>
        ) : null}

        {packages.map((pkg) =>
          renaming === pkg.id ? (
            <Field key={pkg.id}>
              <input
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter")
                    guard(async () => {
                      await api.renamePackage(pkg.id, draft);
                      setRenaming(null);
                    });
                  if (e.key === "Escape") setRenaming(null);
                }}
                aria-label={`Rename ${pkg.name}`}
                autoFocus
              />
            </Field>
          ) : (
            <div className="collrow" key={pkg.id}>
              <button
                type="button"
                className="collrow__main"
                aria-pressed={activePackage === pkg.id}
                onClick={() => onSelect(activePackage === pkg.id ? null : pkg.id)}
                onDoubleClick={() => {
                  setDraft(pkg.name);
                  setRenaming(pkg.id);
                }}
                title={`${pkg.name} (double-click to rename)`}
              >
                <span className="collrow__name">{pkg.name}</span>
                <span className="collrow__count num">
                  {formatCount(pkg.parts)} · {formatBytes(pkg.total_bytes)}
                </span>
              </button>
              <button
                type="button"
                className="collrow__x"
                aria-label={`Ungroup ${pkg.name}`}
                title="Ungroup. The files are not touched."
                onClick={() =>
                  guard(async () => {
                    await api.deletePackage(pkg.id);
                    if (activePackage === pkg.id) onSelect(null);
                  })
                }
              >
                &times;
              </button>
            </div>
          )
        )}

        {suggestions.length > 0 ? (
          <>
            <div className="suggesthead">
              <span>Looks like one model</span>
              <span className="num dim">{formatCount(suggestions.length)}</span>
            </div>
            {visible.map((s) => (
              <div className="suggest" key={s.root}>
                <div className="suggest__text">
                  <span className="suggest__name">{s.name}</span>
                  <span className="dim num">
                    {s.parts} files
                    {nameCounts[s.name] > 1 ? ` · ${folderOf(s.root)}` : ""}
                  </span>
                </div>
                <Button
                  size="sm"
                  disabled={busy === s.root}
                  onClick={() => combine(s)}
                  title={s.root}
                >
                  Group
                </Button>
              </div>
            ))}
            {suggestions.length > 4 ? (
              <Button variant="ghost" size="sm" onClick={() => setShowAll((v) => !v)}>
                {showAll ? "Show fewer" : `Show all ${formatCount(suggestions.length)}`}
              </Button>
            ) : null}
          </>
        ) : null}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
