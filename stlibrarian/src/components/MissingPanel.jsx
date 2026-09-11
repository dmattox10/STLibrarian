import { useEffect, useState } from "react";
import { Alert, Button, Field, FolderIcon, Panel } from "./controls/index.jsx";
import { formatCount } from "../lib/format.js";

/**
 * Files the library has lost track of.
 *
 * Every row stores an absolute path, which is fine until a library moves to a
 * bigger disk, a share remounts somewhere else, or a backup is restored onto a
 * machine that keeps its models in a different place. Then every path is wrong
 * at once and the library looks empty though nothing was lost.
 *
 * Searching matches on content, so it does not care what anything is called or
 * where it sits now. Nothing here deletes a row: a drive that is merely
 * unplugged must not empty somebody's library.
 */
export default function MissingPanel({ api, stats, onChanged }) {
  const [models, setModels] = useState([]);
  const [root, setRoot] = useState("");
  const [plan, setPlan] = useState(null);
  const [report, setReport] = useState(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const missing = stats?.missing ?? 0;

  useEffect(() => {
    if (missing === 0) {
      setModels([]);
      return undefined;
    }
    let cancelled = false;
    api
      .missingModels()
      .then((m) => !cancelled && setModels(m))
      .catch(() => !cancelled && setModels([]));
    return () => {
      cancelled = true;
    };
  }, [api, missing]);

  // Nothing lost, nothing to say.
  if (missing === 0) return null;

  const guard = async (action) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(e?.message || "That did not work.");
    } finally {
      setBusy(false);
    }
  };

  const search = () =>
    guard(async () => {
      setReport(null);
      setPlan(await api.relinkPlan(root.trim()));
    });

  const reconnect = () =>
    guard(async () => {
      setReport(await api.relinkApply(root.trim()));
      setPlan(null);
      onChanged();
    });

  const locate = (model) =>
    guard(async () => {
      const picked = api.pickFile ? await api.pickFile() : null;
      if (!picked) return;
      await api.repointModel(model.id, picked);
      onChanged();
    });

  const browse = () =>
    guard(async () => {
      const picked = await api.pickFolder();
      if (picked) setRoot(picked);
    });

  return (
    <Panel title="Missing files">
      <div className="stack">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          <strong style={{ color: "var(--nord13)" }}>{formatCount(missing)}</strong>{" "}
          {missing === 1 ? "model is" : "models are"} not where the library left
          them. Point it at where they are now and it will match them by content
          — names and folders can have changed.
        </span>

        <Field icon={<FolderIcon />}>
          <input
            value={root}
            placeholder={api.pickFolder ? "Where to look" : "/volume1/3dprints"}
            onChange={(e) => setRoot(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && root.trim() && search()}
            aria-label="Where to look for the missing files"
          />
        </Field>

        <div className="row">
          {api.pickFolder ? (
            <Button size="sm" onClick={browse} disabled={busy}>
              Browse
            </Button>
          ) : null}
          <div style={{ flex: 1 }} />
          <Button size="sm" onClick={search} disabled={busy || !root.trim()}>
            {busy && !plan ? "Looking" : "Search"}
          </Button>
          {plan && plan.length > 0 ? (
            <Button variant="primary" size="sm" onClick={reconnect} disabled={busy}>
              Reconnect {formatCount(plan.length)}
            </Button>
          ) : null}
        </div>

        {plan && plan.length === 0 ? (
          <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
            Nothing there matched. Try a folder further up, or locate one by hand
            below.
          </span>
        ) : null}

        {plan && plan.length > 0
          ? plan.slice(0, 4).map((m) => (
              <span className="organiserow" key={m.model_id} title={`${m.was}\n→ ${m.found}`}>
                {m.found}
              </span>
            ))
          : null}

        {report ? (
          <span className="num" style={{ fontSize: 11, color: "var(--nord14)" }}>
            Reconnected {formatCount(report.relinked)}
            {report.still_missing
              ? `, ${formatCount(report.still_missing)} still missing`
              : ""}
          </span>
        ) : null}

        {models.length > 0 ? (
          <>
            <div className="suggesthead">
              <span>Still lost</span>
              <span className="num dim">{formatCount(models.length)}</span>
            </div>
            {models.slice(0, 6).map((m) => (
              <div className="suggest" key={m.id}>
                <div className="suggest__text">
                  <span className="suggest__name">{m.name}</span>
                  <span className="dim num">{m.path}</span>
                </div>
                {api.pickFile ? (
                  <Button size="sm" disabled={busy} onClick={() => locate(m)}>
                    Locate
                  </Button>
                ) : null}
              </div>
            ))}
            {models.length > 6 ? (
              <span className="dim num" style={{ fontSize: 10.5 }}>
                and {formatCount(models.length - 6)} more
              </span>
            ) : null}
          </>
        ) : null}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
