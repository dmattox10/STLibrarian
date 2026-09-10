import { useEffect, useState } from "react";
import { Alert, Button, Field, FolderIcon, Meter, Panel, TagChip } from "./controls/index.jsx";
import CollectionsPanel from "./CollectionsPanel.jsx";
import { formatBytes, formatCount } from "../lib/format.js";

function ScanPanel({ api, onComplete }) {
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState(null);
  const [report, setReport] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => api.onScanProgress(setProgress), [api]);

  const run = async () => {
    if (!path.trim()) return;
    setBusy(true);
    setError(null);
    setReport(null);
    setProgress(null);
    try {
      const result = await api.scan(path.trim());
      setReport(result);
      onComplete();
    } catch (e) {
      setError(e?.message || "Scan failed.");
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  const browse = async () => {
    const picked = await api.pickFolder();
    if (picked) setPath(picked);
  };

  const seen = progress
    ? progress.added + progress.updated + progress.unchanged + progress.failed
    : 0;

  return (
    <Panel title="Add to library">
      <div className="stack">
        <Field icon={<FolderIcon />}>
          <input
            value={path}
            placeholder={api.mode === "http" ? "/volume1/3dprints" : "Folder to scan"}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && run()}
            aria-label="Folder to scan"
          />
        </Field>
        <div className="row">
          {api.mode !== "http" ? (
            <Button size="sm" onClick={browse} disabled={busy}>
              Browse
            </Button>
          ) : null}
          <div style={{ flex: 1 }} />
          <Button variant="primary" size="sm" onClick={run} disabled={busy || !path.trim()}>
            {busy ? "Scanning" : "Scan"}
          </Button>
        </div>

        {busy ? (
          <div className="stack" style={{ gap: 5 }}>
            <Meter busy label="Scanning library" />
            <span className="dim num" style={{ fontSize: 11 }}>
              {seen > 0 ? `${formatCount(seen)} files indexed…` : "Walking folders…"}
            </span>
          </div>
        ) : null}

        {report ? (
          <span className="dim num" style={{ fontSize: 11 }}>
            +{report.added} new · {report.updated} updated · {report.unchanged} unchanged
            {report.failed ? ` · ${report.failed} failed` : ""}
          </span>
        ) : null}

        <Alert>{error}</Alert>

        {api.mode === "http" ? (
          <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
            Paths are read on the server, not on this computer.
          </span>
        ) : null}
      </div>
    </Panel>
  );
}

export default function Sidebar({
  api,
  tags,
  activeTags,
  onToggleTag,
  collections,
  activeCollection,
  onSelectCollection,
  stats,
  onRefresh,
  canScan,
}) {
  return (
    <>
      {canScan ? <ScanPanel api={api} onComplete={onRefresh} /> : null}

      <CollectionsPanel
        api={api}
        collections={collections}
        activeCollection={activeCollection}
        onSelect={onSelectCollection}
        onChanged={onRefresh}
      />

      <Panel
        title="Tags"
        actions={
          activeTags.length ? (
            <Button variant="ghost" size="sm" onClick={() => activeTags.forEach(onToggleTag)}>
              Clear
            </Button>
          ) : null
        }
      >
        {tags.length === 0 ? (
          <span className="dim" style={{ fontSize: 12 }}>
            Tags appear here once you have scanned a folder.
          </span>
        ) : (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
            {tags.map((tag) => (
              <TagChip
                key={tag.name}
                name={tag.name}
                count={tag.count}
                pressed={activeTags.includes(tag.name)}
                onClick={() => onToggleTag(tag.name)}
              />
            ))}
          </div>
        )}
      </Panel>

      <Panel title="Library">
        <dl className="gc-kv">
          <dt>Models</dt>
          <dd>{formatCount(stats.models)}</dd>
          <dt>On disk</dt>
          <dd>{formatBytes(stats.total_bytes)}</dd>
          <dt>Tags</dt>
          <dd>{formatCount(stats.tags)}</dd>
          {stats.missing > 0 ? (
            <>
              <dt>Missing</dt>
              <dd style={{ color: "var(--nord13)" }}>{formatCount(stats.missing)}</dd>
            </>
          ) : null}
        </dl>
        {stats.missing > 0 ? (
          <p className="dim" style={{ fontSize: 11, lineHeight: 1.5, margin: "8px 0 0" }}>
            {stats.missing} indexed file{stats.missing === 1 ? " is" : "s are"} no longer on disk.
            They are kept in case a drive is simply unplugged.
          </p>
        ) : null}
      </Panel>
    </>
  );
}
