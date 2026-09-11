import { useEffect, useState } from "react";
import StlViewer from "./StlViewer.jsx";
import PrintLog from "./PrintLog.jsx";
import SlicesPanel from "./SlicesPanel.jsx";
import ProvenancePanel from "./ProvenancePanel.jsx";
import RenderPicker from "./RenderPicker.jsx";
import { Alert, Button, CloseIcon, Field, Panel, TagChip, TrashIcon } from "./controls/index.jsx";
import {
  fitsBed,
  formatBytes,
  formatCount,
  formatDate,
  formatDims,
  formatVolume,
  formatWeight,
  volumeLooksReliable,
} from "../lib/format.js";

function TagEditor({ model, api, onChanged }) {
  const [draft, setDraft] = useState("");
  const [tags, setTags] = useState(model.tags);
  const [error, setError] = useState(null);

  useEffect(() => {
    setTags(model.tags);
    setDraft("");
    setError(null);
  }, [model.id, model.tags]);

  const commit = async (next) => {
    const previous = tags;
    setTags(next); // optimistic -- tagging should feel instant
    setError(null);
    try {
      await api.setTags(model.id, next);
      onChanged();
    } catch (e) {
      setTags(previous);
      setError(e?.message || "Could not save tags.");
    }
  };

  const add = () => {
    const value = draft.trim();
    // Commas separate tags in the Pro server's query string, so a tag
    // containing one could never be filtered for again.
    const cleaned = value.replace(/,/g, " ").trim();
    if (!cleaned || tags.includes(cleaned)) {
      setDraft("");
      return;
    }
    commit([...tags, cleaned]);
    setDraft("");
  };

  return (
    <div className="stack">
      <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
        {tags.length === 0 ? (
          <span className="dim" style={{ fontSize: 12 }}>
            No tags yet.
          </span>
        ) : (
          tags.map((tag) => (
            <TagChip
              key={tag}
              name={tag}
              onRemove={() => commit(tags.filter((t) => t !== tag))}
            />
          ))
        )}
      </div>
      <Field>
        <input
          value={draft}
          placeholder="Add a tag"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }}
          aria-label="Add a tag"
        />
        <Button size="sm" onClick={add} disabled={!draft.trim()}>
          Add
        </Button>
      </Field>
      <Alert>{error}</Alert>
    </div>
  );
}

export default function Inspector({ api, model, onClose, onChanged, canDelete }) {
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    setConfirming(false);
    setError(null);
  }, [model?.id]);

  if (!model) return null;

  const bed = fitsBed(model);

  const remove = async () => {
    try {
      await api.deleteModel(model.id);
      onChanged();
      onClose();
    } catch (e) {
      setError(e?.message || "Could not remove this model.");
    }
  };

  return (
    <aside className="inspector">
      <div className="row">
        <h2
          style={{
            margin: 0,
            fontSize: 14,
            color: "var(--ink-bright)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
          title={model.name}
        >
          {model.name}
        </h2>
        <div style={{ flex: 1 }} />
        <Button variant="ghost" icon onClick={onClose} aria-label="Close inspector">
          <CloseIcon />
        </Button>
      </div>

      <StlViewer api={api} model={model} />

      {api.openInSlicer || api.revealInFolder ? (
        <div className="row">
          {api.openInSlicer ? (
            <Button
              size="sm"
              onClick={() => api.openInSlicer(model.id).catch((e) => setError(e?.message))}
              title="Open with whatever handles this file type"
            >
              Open in slicer
            </Button>
          ) : null}
          {api.revealInFolder ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => api.revealInFolder(model.id).catch((e) => setError(e?.message))}
            >
              Show in Finder
            </Button>
          ) : null}
        </div>
      ) : null}

      <Panel title="Details">
        <dl className="gc-kv">
          <dt>Size</dt>
          <dd>{formatBytes(model.size)}</dd>
          <dt>Triangles</dt>
          <dd>{formatCount(model.triangles)}</dd>
          {model.variant ? (
            <>
              <dt>Variant</dt>
              <dd
                style={{
                  color:
                    model.variant === "supported" ? "var(--nord14)" : "var(--nord9)",
                }}
              >
                {model.variant}
              </dd>
            </>
          ) : null}
          <dt>Bounds</dt>
          <dd>{formatDims(model)}</dd>
          {formatVolume(model.volume) ? (
            <>
              <dt>Volume</dt>
              <dd
                title={
                  volumeLooksReliable(model)
                    ? "Solid volume, from a closed mesh"
                    : "The mesh is not closed, so this is not the volume of anything"
                }
                style={
                  volumeLooksReliable(model) ? undefined : { color: "var(--nord13)" }
                }
              >
                {formatVolume(model.volume)}
                {volumeLooksReliable(model) ? "" : " ?"}
              </dd>
              {volumeLooksReliable(model) ? (
                <>
                  <dt>Resin</dt>
                  <dd title="Solid, so an upper bound">
                    &asymp; {formatWeight(model.volume)}
                  </dd>
                </>
              ) : null}
            </>
          ) : null}
          {bed !== null ? (
            <>
              <dt>220 bed</dt>
              <dd style={{ color: bed ? "var(--nord14)" : "var(--nord13)" }}>
                {bed ? "Fits" : "Too large"}
              </dd>
            </>
          ) : null}
          {model.print_count > 0 ? (
            <>
              <dt>Printed</dt>
              <dd>
                {model.print_count}x
                {model.last_printed ? `, last ${formatDate(model.last_printed)}` : ""}
              </dd>
            </>
          ) : null}
          <dt>Mesh</dt>
          <dd style={{ color: model.watertight ? "var(--nord14)" : "var(--nord13)" }}>
            {model.watertight ? "Closed" : "Has holes"}
          </dd>
          <dt>Modified</dt>
          <dd>{formatDate(model.mtime)}</dd>
          <dt>Path</dt>
          <dd style={{ fontSize: 10.5 }}>{model.path}</dd>
        </dl>
      </Panel>

      <Panel title="Tags">
        <TagEditor model={model} api={api} onChanged={onChanged} />
      </Panel>

      <ProvenancePanel api={api} model={model} onChanged={onChanged} />

      <RenderPicker api={api} model={model} onChanged={onChanged} />

      <SlicesPanel api={api} model={model} onChanged={onChanged} />

      <PrintLog api={api} model={model} onChanged={onChanged} />

      <Panel title="Collections">
        {model.collections?.length ? (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {model.collections.map((name) => (
              <span className="gc-badge" key={name}>
                {name}
              </span>
            ))}
          </div>
        ) : (
          <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
            Not in any collection. Select models in the grid to add them.
          </span>
        )}
      </Panel>

      {canDelete ? (
        <Panel title="Danger zone">
          <div className="stack">
            <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
              Removes this model from the library index. The STL file on disk is
              left alone.
            </span>
            {confirming ? (
              <div className="row">
                <Button variant="danger" size="sm" onClick={remove}>
                  <TrashIcon />
                  Really remove
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
                  Cancel
                </Button>
              </div>
            ) : (
              <Button variant="danger" size="sm" onClick={() => setConfirming(true)}>
                <TrashIcon />
                Remove from library
              </Button>
            )}
            <Alert>{error}</Alert>
          </div>
        </Panel>
      ) : null}
    </aside>
  );
}
