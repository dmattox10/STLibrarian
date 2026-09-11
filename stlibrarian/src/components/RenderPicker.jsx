import { useEffect, useState } from "react";
import { Alert, Button, Panel } from "./controls/index.jsx";

const fileName = (path) => path.split("/").pop();

/**
 * Which picture represents this model.
 *
 * A creator's own render is posed, lit and often painted, so it is chosen
 * automatically when one sits beside the file. This is for when the automatic
 * choice picked the group shot and you wanted the close-up.
 */
export default function RenderPicker({ api, model, onChanged }) {
  const [options, setOptions] = useState([]);
  const [error, setError] = useState(null);

  useEffect(() => {
    let cancelled = false;
    api
      .modelRenders(model.id)
      .then((o) => !cancelled && setOptions(o))
      .catch(() => !cancelled && setOptions([]));
    setError(null);
    return () => {
      cancelled = true;
    };
  }, [api, model.id]);

  // Nothing to choose between, so nothing worth showing.
  if (options.length === 0) return null;

  const choose = async (imageId) => {
    setError(null);
    try {
      await api.setModelRender(model.id, imageId);
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not change the preview.");
    }
  };

  return (
    <Panel title="Preview">
      <div className="stack">
        <div className="renderopts">
          <button
            type="button"
            className="renderopt"
            aria-pressed={!model.render}
            onClick={() => choose(null)}
            title="Draw it from the mesh instead"
          >
            Generated
          </button>
          {options.map(([id, path]) => (
            <button
              type="button"
              className="renderopt"
              key={id}
              aria-pressed={model.render === path}
              onClick={() => choose(id)}
              title={path}
            >
              {fileName(path)}
            </button>
          ))}
        </div>
        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
