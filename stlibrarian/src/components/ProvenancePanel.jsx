import { useEffect, useState } from "react";
import { Alert, Button, Field, Panel } from "./controls/index.jsx";

/**
 * Where a model came from.
 *
 * Guessed from the folder layout when scanned, and freely editable -- a wrong
 * guess costs one correction, whereas an empty field stays empty across nine
 * hundred models. A rescan never overwrites what you have fixed.
 */
export default function ProvenancePanel({ api, model, onChanged }) {
  const [creator, setCreator] = useState(model.creator || "");
  const [source, setSource] = useState(model.source || "");
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState(null);

  useEffect(() => {
    setCreator(model.creator || "");
    setSource(model.source || "");
    setSaved(false);
    setError(null);
  }, [model.id, model.creator, model.source]);

  const dirty = creator !== (model.creator || "") || source !== (model.source || "");

  const save = async () => {
    setError(null);
    try {
      await api.setProvenance(model.id, creator, source);
      setSaved(true);
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not save that.");
    }
  };

  return (
    <Panel title="Origin">
      <div className="stack">
        <Field>
          <input
            value={creator}
            onChange={(e) => {
              setCreator(e.target.value);
              setSaved(false);
            }}
            placeholder="Creator"
            aria-label="Creator"
            onKeyDown={(e) => e.key === "Enter" && dirty && save()}
          />
        </Field>
        <Field>
          <input
            value={source}
            onChange={(e) => {
              setSource(e.target.value);
              setSaved(false);
            }}
            placeholder="Source: a Patreon month, a bundle, a site"
            aria-label="Source"
            onKeyDown={(e) => e.key === "Enter" && dirty && save()}
          />
        </Field>
        <div className="row">
          <Button size="sm" onClick={save} disabled={!dirty}>
            Save
          </Button>
          {saved && !dirty ? (
            <span className="dim" style={{ fontSize: 11 }}>
              Saved
            </span>
          ) : null}
        </div>
        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
