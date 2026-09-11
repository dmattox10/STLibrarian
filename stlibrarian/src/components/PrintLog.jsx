import { useEffect, useState } from "react";
import { Alert, Button, Field, Panel, Segmented, TrashIcon } from "./controls/index.jsx";
import { formatDate } from "../lib/format.js";

const OUTCOMES = [
  { value: "success", label: "Worked" },
  { value: "partial", label: "Partly" },
  { value: "failed", label: "Failed" },
];

const OUTCOME_COLOR = {
  success: "var(--nord14)",
  partial: "var(--nord13)",
  failed: "var(--nord11)",
};

/**
 * What you actually printed, and how it went.
 *
 * The form stays short on purpose. A log with ten required fields is a log
 * nobody keeps, so everything but the outcome is optional.
 */
export default function PrintLog({ api, model, onChanged }) {
  const [history, setHistory] = useState([]);
  const [adding, setAdding] = useState(false);
  const [outcome, setOutcome] = useState("success");
  const [printer, setPrinter] = useState("");
  const [material, setMaterial] = useState("");
  const [settings, setSettings] = useState("");
  const [notes, setNotes] = useState("");
  const [error, setError] = useState(null);

  const load = () =>
    api
      .prints(model.id)
      .then(setHistory)
      .catch(() => setHistory([]));

  useEffect(() => {
    let cancelled = false;
    api
      .prints(model.id)
      .then((h) => !cancelled && setHistory(h))
      .catch(() => !cancelled && setHistory([]));
    setAdding(false);
    setError(null);
    return () => {
      cancelled = true;
    };
  }, [api, model.id]);

  const submit = async () => {
    setError(null);
    try {
      await api.logPrint(model.id, { outcome, printer, material, settings, notes });
      // Remember the printer and material: the next print is usually on the
      // same machine with the same bottle.
      setSettings("");
      setNotes("");
      setAdding(false);
      await load();
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not save that.");
    }
  };

  const remove = async (id) => {
    try {
      await api.deletePrint(id);
      await load();
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not remove that.");
    }
  };

  return (
    <Panel
      title="Prints"
      actions={
        <Button variant="ghost" size="sm" onClick={() => setAdding((v) => !v)}>
          {adding ? "Cancel" : "Log one"}
        </Button>
      }
    >
      <div className="stack">
        {adding ? (
          <div className="stack" style={{ gap: 7 }}>
            <Segmented value={outcome} onChange={setOutcome} options={OUTCOMES} label="Outcome" />
            <Field>
              <input
                value={printer}
                onChange={(e) => setPrinter(e.target.value)}
                placeholder="Printer"
                aria-label="Printer"
              />
            </Field>
            <Field>
              <input
                value={material}
                onChange={(e) => setMaterial(e.target.value)}
                placeholder="Material"
                aria-label="Material"
              />
            </Field>
            <Field>
              <input
                value={settings}
                onChange={(e) => setSettings(e.target.value)}
                placeholder="Settings — layer height, exposure, supports"
                aria-label="Settings"
              />
            </Field>
            <Field>
              <input
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder="Notes"
                aria-label="Notes"
                onKeyDown={(e) => e.key === "Enter" && submit()}
              />
            </Field>
            <Button variant="primary" size="sm" onClick={submit}>
              Save print
            </Button>
          </div>
        ) : null}

        {history.length === 0 && !adding ? (
          <span className="dim" style={{ fontSize: 12, lineHeight: 1.5 }}>
            Never printed.
          </span>
        ) : null}

        {history.map((p) => (
          <div className="printrow" key={p.id}>
            <span
              className="printrow__dot"
              style={{ background: OUTCOME_COLOR[p.outcome] || "var(--nord3)" }}
              title={p.outcome}
            />
            <div className="printrow__body">
              <div className="printrow__top">
                <span>{formatDate(p.printed_at)}</span>
                {p.printer ? <span className="dim">{p.printer}</span> : null}
              </div>
              {p.material || p.settings ? (
                <span className="dim num printrow__meta">
                  {[p.material, p.settings].filter(Boolean).join(" · ")}
                </span>
              ) : null}
              {p.notes ? <span className="printrow__notes">{p.notes}</span> : null}
            </div>
            <button
              type="button"
              className="collrow__x"
              aria-label="Remove this print"
              onClick={() => remove(p.id)}
            >
              <TrashIcon />
            </button>
          </div>
        ))}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
