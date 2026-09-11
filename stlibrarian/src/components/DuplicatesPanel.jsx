import { useEffect, useState } from "react";
import { Button, Panel } from "./controls/index.jsx";
import { formatBytes } from "../lib/format.js";

/**
 * Byte-for-byte copies of the same model.
 *
 * Reports only. Deleting files is not something the library does -- that is
 * the Pro server's job, deliberately, so the desktop client never writes to
 * your disk.
 */
export default function DuplicatesPanel({ api, onSelectModel, reloadToken }) {
  const [groups, setGroups] = useState([]);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api
      .duplicates()
      .then((d) => !cancelled && setGroups(d))
      .catch(() => !cancelled && setGroups([]));
    return () => {
      cancelled = true;
    };
  }, [api, reloadToken]);

  if (groups.length === 0) return null;

  const wasted = groups.reduce((sum, g) => sum + g.wasted_bytes, 0);

  return (
    <Panel
      title="Duplicates"
      actions={
        <Button variant="ghost" size="sm" onClick={() => setOpen((v) => !v)}>
          {open ? "Hide" : "Show"}
        </Button>
      }
    >
      <div className="stack">
        <span className="dim" style={{ fontSize: 12, lineHeight: 1.5 }}>
          {groups.length} model{groups.length === 1 ? "" : "s"} stored more than
          once. <strong style={{ color: "var(--nord13)" }}>{formatBytes(wasted)}</strong>{" "}
          would come back by keeping one copy of each.
        </span>

        {open
          ? groups.map((g) => (
              <div className="dupe" key={g.hash}>
                <div className="dupe__head">
                  <span className="dupe__name">{g.name}</span>
                  <span className="num dim">
                    {g.copies} copies · {formatBytes(g.wasted_bytes)}
                  </span>
                </div>
                {g.models.map((m) => (
                  <button
                    type="button"
                    className="dupe__path"
                    key={m.id}
                    onClick={() => onSelectModel(m)}
                    title={m.path}
                  >
                    {m.path}
                  </button>
                ))}
              </div>
            ))
          : null}
      </div>
    </Panel>
  );
}
