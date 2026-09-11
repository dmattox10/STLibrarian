import { useEffect, useState } from "react";
import { Button, Panel } from "./controls/index.jsx";
import { formatBytes } from "../lib/format.js";

/**
 * What to copy if you want to keep this.
 *
 * Tags, packages, print history and every correction you have made live in one
 * SQLite file. The models themselves are wherever you keep them and are not
 * the library's to look after -- but everything you have *told* the library is
 * here, and nowhere else.
 */
export default function BackupPanel({ api }) {
  const [where, setWhere] = useState(null);

  useEffect(() => {
    let cancelled = false;
    api
      .libraryLocation()
      .then((l) => !cancelled && setWhere(l))
      // A non-administrator on a server has no business seeing its paths.
      .catch(() => !cancelled && setWhere(null));
    return () => {
      cancelled = true;
    };
  }, [api]);

  if (!where) return null;

  return (
    <Panel
      title="Backup"
      actions={
        api.revealDataDir ? (
          <Button variant="ghost" size="sm" onClick={() => api.revealDataDir()}>
            Reveal
          </Button>
        ) : null
      }
    >
      <div className="stack">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          Everything you have told this library is in one file: tags, packages,
          print history, every correction. Copy it and you have kept all of it.
        </span>

        <div className="backup">
          <span className="backup__label">Database</span>
          <span className="backup__path num" title={where.db_path}>
            {where.db_path}
          </span>
          <span className="backup__size num">{formatBytes(where.db_bytes)}</span>
        </div>

        <div className="backup">
          <span className="backup__label">Previews</span>
          <span className="backup__path num" title={where.thumbs_dir}>
            {where.thumbs_dir}
          </span>
          <span className="backup__size num">{formatBytes(where.thumbs_bytes)}</span>
        </div>

        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          Previews are drawn from the models and rebuild themselves, so a backup
          that skips them loses nothing but time. Close the app before copying,
          or you may catch a write half-finished.
          {where.overridden ? " This location was set by STLIBRARIAN_DATA_DIR." : ""}
        </span>
      </div>
    </Panel>
  );
}
