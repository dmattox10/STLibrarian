import { useState } from "react";
import { Button, CloseIcon, Field } from "./controls/index.jsx";
import { formatCount } from "../lib/format.js";

/**
 * Actions that apply to a selection rather than to one model.
 *
 * Appears only when something is selected, so it costs nothing the rest of the
 * time, and reports what actually changed -- "tagged 12" is different from
 * "tagged 40 of which 28 already had it".
 */
export default function BulkBar({ api, ids, collections, onDone, onClear, onSelectAll, total }) {
  const [tag, setTag] = useState("");
  const [collectionId, setCollectionId] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState(null);
  const [error, setError] = useState(null);

  if (ids.length === 0) return null;

  const run = async (action, describe) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const changed = await action();
      const count = typeof changed === "number" ? changed : changed?.changed ?? 0;
      setNote(describe(count));
      onDone();
    } catch (e) {
      setError(e?.message || "That did not work.");
    } finally {
      setBusy(false);
    }
  };

  const cleanTag = tag.replace(/,/g, " ").trim();

  return (
    <div className="bulkbar" role="region" aria-label="Bulk actions">
      <span className="bulkbar__count">{formatCount(ids.length)} selected</span>

      <div className="bulkbar__group">
        <Field style={{ width: 150 }}>
          <input
            value={tag}
            placeholder="Tag"
            onChange={(e) => setTag(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && cleanTag) {
                run(
                  () => api.bulkAddTag(ids, cleanTag),
                  (n) => `Tagged ${n}`
                );
              }
            }}
            aria-label="Tag to apply or remove"
          />
        </Field>
        <Button
          size="sm"
          disabled={busy || !cleanTag}
          onClick={() =>
            run(
              () => api.bulkAddTag(ids, cleanTag),
              (n) => (n === 0 ? "All already tagged" : `Tagged ${n}`)
            )
          }
        >
          Add
        </Button>
        <Button
          size="sm"
          disabled={busy || !cleanTag}
          onClick={() =>
            run(
              () => api.bulkRemoveTag(ids, cleanTag),
              (n) => (n === 0 ? "None had that tag" : `Untagged ${n}`)
            )
          }
        >
          Remove
        </Button>
      </div>

      <div className="bulkbar__group">
        <Field style={{ width: 168 }}>
          <select
            value={collectionId}
            onChange={(e) => setCollectionId(e.target.value)}
            aria-label="Collection"
          >
            <option value="">Collection…</option>
            {collections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </Field>
        <Button
          variant="primary"
          size="sm"
          disabled={busy || !collectionId}
          onClick={() =>
            run(
              () => api.addToCollection(Number(collectionId), ids),
              (n) => (n === 0 ? "Already in it" : `Added ${n}`)
            )
          }
        >
          Add to
        </Button>
        <Button
          size="sm"
          disabled={busy || !collectionId}
          onClick={() =>
            run(
              () => api.removeFromCollection(Number(collectionId), ids),
              (n) => (n === 0 ? "None were in it" : `Removed ${n}`)
            )
          }
        >
          Take out
        </Button>
      </div>

      <div className="bulkbar__spacer" />

      {note ? <span className="bulkbar__note">{note}</span> : null}
      {error ? <span className="bulkbar__note bulkbar__note--error">{error}</span> : null}

      {ids.length < total ? (
        <Button variant="ghost" size="sm" onClick={onSelectAll}>
          Select all {formatCount(total)}
        </Button>
      ) : null}
      <Button variant="ghost" icon onClick={onClear} aria-label="Clear selection">
        <CloseIcon />
      </Button>
    </div>
  );
}
