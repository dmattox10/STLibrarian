import { useState } from "react";
import { Alert, Button, Field, Panel } from "./controls/index.jsx";
import { formatCount } from "../lib/format.js";

/**
 * Rearrange the library on disk. Pro only.
 *
 * Everything else in this app reads. This writes, so it shows you the whole
 * plan first and moves nothing until you say so -- and it reports conflicts
 * rather than resolving them, because silently overwriting somebody's model is
 * not a thing software should do on its own.
 */
export default function OrganizePanel({ api, onChanged }) {
  const [root, setRoot] = useState("");
  const [plan, setPlan] = useState(null);
  const [report, setReport] = useState(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState(null);

  if (!api.organizePlan) return null;

  const preview = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    setConfirming(false);
    try {
      setPlan(await api.organizePlan(root.trim()));
    } catch (e) {
      setError(e?.message || "Could not work out a plan.");
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await api.organizeApply(root.trim()));
      setPlan(null);
      setConfirming(false);
      onChanged();
    } catch (e) {
      setError(e?.message || "Could not organise the library.");
    } finally {
      setBusy(false);
    }
  };

  const moves = plan?.filter((p) => p.action === "move") ?? [];
  const conflicts = plan?.filter((p) => p.action === "conflict") ?? [];
  const inPlace = plan?.filter((p) => p.action === "inplace") ?? [];

  return (
    <Panel title="Organise on disk">
      <div className="stack">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          Files are filed as creator / model / variant. Nothing is overwritten,
          and nothing moves until you apply.
        </span>

        <Field>
          <input
            value={root}
            placeholder="/volume1/3dprints"
            onChange={(e) => setRoot(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && root.trim() && preview()}
            aria-label="Where to file the library"
          />
        </Field>

        <div className="row">
          <Button size="sm" onClick={preview} disabled={busy || !root.trim()}>
            {busy && !confirming ? "Working" : "Preview"}
          </Button>
          {plan && moves.length > 0 ? (
            confirming ? (
              <>
                <Button variant="danger" size="sm" onClick={apply} disabled={busy}>
                  Move {formatCount(moves.length)} files
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
                  Cancel
                </Button>
              </>
            ) : (
              <Button variant="primary" size="sm" onClick={() => setConfirming(true)}>
                Apply
              </Button>
            )
          ) : null}
        </div>

        {plan ? (
          <div className="stack" style={{ gap: 4 }}>
            <span className="num dim" style={{ fontSize: 11 }}>
              {formatCount(moves.length)} to move · {formatCount(inPlace.length)} already
              filed
              {conflicts.length > 0 ? ` · ${formatCount(conflicts.length)} blocked` : ""}
            </span>
            {moves.slice(0, 6).map((m) => (
              <span className="organiserow" key={m.model_id} title={`${m.from}\n→ ${m.to}`}>
                {m.to.replace(root.trim().replace(/\/$/, ""), "") || m.to}
              </span>
            ))}
            {moves.length > 6 ? (
              <span className="dim num" style={{ fontSize: 10.5 }}>
                and {formatCount(moves.length - 6)} more
              </span>
            ) : null}
            {conflicts.length > 0 ? (
              <span style={{ fontSize: 11, color: "var(--nord13)", lineHeight: 1.5 }}>
                Something already sits where those would go. They are left alone.
              </span>
            ) : null}
          </div>
        ) : null}

        {report ? (
          <span className="num" style={{ fontSize: 11, color: "var(--nord14)" }}>
            Moved {formatCount(report.moved)}
            {report.conflicts ? `, ${formatCount(report.conflicts)} blocked` : ""}
            {report.failed ? `, ${formatCount(report.failed)} failed` : ""}
          </span>
        ) : null}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
