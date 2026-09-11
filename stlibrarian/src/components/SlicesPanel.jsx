import { useEffect, useState } from "react";
import { Alert, Button, Panel } from "./controls/index.jsx";
import { formatBytes, formatDuration } from "../lib/format.js";

/**
 * The printable outputs of a model.
 *
 * A model is not printable -- the gap between an STL and a print is slicing,
 * which belongs to Lychee and PrusaSlicer. What a library can do is keep the
 * result, so "the one we sliced for the Mars at 50 microns that worked" is a
 * thing you can find again instead of remember.
 */
export default function SlicesPanel({ api, model, onChanged }) {
  const [slices, setSlices] = useState([]);
  const [error, setError] = useState(null);

  useEffect(() => {
    let cancelled = false;
    api
      .slices(model.id)
      .then((s) => !cancelled && setSlices(s))
      .catch(() => !cancelled && setSlices([]));
    setError(null);
    return () => {
      cancelled = true;
    };
  }, [api, model.id, model.slice_count]);

  if (slices.length === 0) {
    return (
      <Panel title="Sliced">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          Nothing sliced yet. Save a slicer&rsquo;s output beside this file and
          it will be picked up on the next scan.
        </span>
      </Panel>
    );
  }

  const detach = async (id) => {
    setError(null);
    try {
      await api.setSliceOwner(id, null, null);
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not detach that.");
    }
  };

  return (
    <Panel title="Sliced">
      <div className="stack">
        {slices.map((s) => {
          // Resin formats are vendor binaries and are not parsed, so most of
          // these will be blank for a .ctb. Showing nothing is the honest
          // outcome; showing a guess would not be.
          const facts = [
            s.printer,
            s.layer_height ? `${s.layer_height}mm` : null,
            formatDuration(s.print_time_secs),
            s.material_grams ? `${s.material_grams.toFixed(1)} g` : null,
          ].filter(Boolean);

          return (
            <div className="slicerow" key={s.id}>
              <span className="slicerow__fmt">{s.format}</span>
              <div className="slicerow__body">
                <span className="slicerow__name" title={s.path}>
                  {s.name}
                </span>
                <span className="dim num slicerow__meta">
                  {facts.length ? facts.join(" · ") : formatBytes(s.size)}
                  {s.package_id ? " · whole plate" : ""}
                </span>
              </div>
              {api.openSlice ? (
                <Button
                  size="sm"
                  onClick={() => api.openSlice(s.id).catch((e) => setError(e?.message))}
                  title="Open with whatever handles this format"
                >
                  Open
                </Button>
              ) : api.sliceFileUrl ? (
                <a className="gc-btn gc-btn--sm" href={api.sliceFileUrl(s.id)} download>
                  Download
                </a>
              ) : null}
              <button
                type="button"
                className="collrow__x"
                aria-label={`Detach ${s.name}`}
                title="Detach — the file is not touched"
                onClick={() => detach(s.id)}
              >
                &times;
              </button>
            </div>
          );
        })}
        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
