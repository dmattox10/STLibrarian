import { useState } from "react";
import { Alert, Button, Field, Panel, Toggle } from "./controls/index.jsx";
import { formatCount } from "../lib/format.js";

/**
 * Collections: named sets you build by hand.
 *
 * Kept visually distinct from tags in the rail because they are a different
 * idea -- a tag is a property the members share, a collection is simply what
 * you put in it.
 */
export default function CollectionsPanel({
  api,
  collections,
  activeCollection,
  onSelect,
  onChanged,
}) {
  const [creating, setCreating] = useState(false);
  const [draft, setDraft] = useState("");
  const [shared, setShared] = useState(true);
  const [renaming, setRenaming] = useState(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [confirming, setConfirming] = useState(null);
  const [error, setError] = useState(null);

  const guard = async (action) => {
    setError(null);
    try {
      await action();
      onChanged();
    } catch (e) {
      setError(e?.message || "That did not work.");
    }
  };

  const create = () => {
    const name = draft.trim();
    if (!name) return;
    guard(async () => {
      await api.createCollection(name, shared);
      setDraft("");
      setCreating(false);
    });
  };

  const commitRename = (id) => {
    const name = renameDraft.trim();
    if (!name) {
      setRenaming(null);
      return;
    }
    guard(async () => {
      await api.renameCollection(id, name);
      setRenaming(null);
    });
  };

  return (
    <Panel
      title="Collections"
      actions={
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            setCreating((v) => !v);
            setError(null);
          }}
        >
          {creating ? "Cancel" : "New"}
        </Button>
      }
    >
      <div className="stack">
        {creating ? (
          <Field>
            <input
              value={draft}
              placeholder="Collection name"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") create();
                if (e.key === "Escape") setCreating(false);
              }}
              aria-label="New collection name"
              autoFocus
            />
            <Button size="sm" onClick={create} disabled={!draft.trim()}>
              Create
            </Button>
          </Field>
        ) : null}

        {creating && api.needsAuth ? (
          <Toggle checked={!shared} onChange={(v) => setShared(!v)}>
            Just for me
          </Toggle>
        ) : null}

        {collections.length === 0 && !creating ? (
          <span className="dim" style={{ fontSize: 12, lineHeight: 1.5 }}>
            Make a collection, then select models in the grid and add them to it.
          </span>
        ) : null}

        {collections.map((collection) => {
          if (renaming === collection.id) {
            return (
              <Field key={collection.id}>
                <input
                  value={renameDraft}
                  onChange={(e) => setRenameDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename(collection.id);
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  aria-label={`Rename ${collection.name}`}
                  autoFocus
                />
              </Field>
            );
          }

          const active = activeCollection === collection.id;
          return (
            <div className="collrow" key={collection.id}>
              <button
                type="button"
                className="collrow__main"
                aria-pressed={active}
                // Clicking the active one clears the filter, so the rail never
                // becomes a trap you have to hunt for the exit of.
                onClick={() => onSelect(active ? null : collection.id)}
                onDoubleClick={() => {
                  setRenameDraft(collection.name);
                  setRenaming(collection.id);
                }}
                title={`${collection.name} — double-click to rename`}
              >
                <span className="collrow__name">
                  {collection.name}
                  {collection.owner_id != null ? (
                    <span className="collrow__mine" title="Only you can see this">
                      {" "}
                      &#9679;
                    </span>
                  ) : null}
                </span>
                <span className="collrow__count num">{formatCount(collection.count)}</span>
              </button>
              <button
                type="button"
                className="collrow__x"
                aria-label={`Delete collection ${collection.name}`}
                onClick={() => setConfirming(collection.id)}
              >
                &times;
              </button>
            </div>
          );
        })}

        {confirming != null ? (
          <div className="stack" style={{ gap: 6 }}>
            <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
              Delete this collection? The models in it are not touched.
            </span>
            <div className="row">
              <Button
                variant="danger"
                size="sm"
                onClick={() =>
                  guard(async () => {
                    await api.deleteCollection(confirming);
                    if (activeCollection === confirming) onSelect(null);
                    setConfirming(null);
                  })
                }
              >
                Delete
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setConfirming(null)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : null}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
