import { useState } from "react";
import { Alert, Button, Field, Panel } from "./controls/index.jsx";
import { api, isDesktop, readRemote, writeRemote } from "../data/adapter.js";
import { formatBytes } from "../lib/format.js";

/**
 * Which library this desktop client is reading.
 *
 * Its own disk, or somebody's server. Connecting to a server is worth doing
 * from the desktop app rather than a browser for exactly one reason: a browser
 * can offer you a download, but it cannot put the file into your slicer. When
 * connected, that still works -- the file is fetched and handed to whatever
 * opens it here.
 */
export default function ServerPanel() {
  const [url, setUrl] = useState(readRemote() || "");
  const [freed, setFreed] = useState(null);
  const [error, setError] = useState(null);

  // Browsers are already talking to a server; there is nothing to choose.
  if (!isDesktop()) return null;

  const remote = readRemote();

  const connect = () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    if (!/^https?:\/\//i.test(trimmed)) {
      setError("Start with http:// or https://");
      return;
    }
    writeRemote(trimmed);
    // The adapter is chosen once at startup, so switching library means
    // starting again. Simpler and more predictable than rebuilding it live.
    window.location.reload();
  };

  const disconnect = () => {
    writeRemote(null);
    window.location.reload();
  };

  return (
    <Panel title="Library">
      <div className="stack">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          {remote ? (
            <>
              Reading <strong style={{ color: "var(--nord8)" }}>{remote}</strong>.
              Files open in your slicer here, not on the server.
            </>
          ) : (
            "Reading this computer's disk."
          )}
        </span>

        {remote ? (
          <div className="row">
            <Button size="sm" onClick={disconnect}>
              Use this computer
            </Button>
            {api.clearCache ? (
              <Button
                variant="ghost"
                size="sm"
                title="Delete files downloaded from the server"
                onClick={() =>
                  api
                    .clearCache()
                    .then((bytes) => setFreed(bytes))
                    .catch((e) => setError(e?.message))
                }
              >
                Clear cache
              </Button>
            ) : null}
          </div>
        ) : (
          <>
            <Field>
              <input
                value={url}
                placeholder="http://nas.local:3000"
                onChange={(e) => {
                  setUrl(e.target.value);
                  setError(null);
                }}
                onKeyDown={(e) => e.key === "Enter" && connect()}
                aria-label="Server address"
              />
            </Field>
            <Button size="sm" onClick={connect} disabled={!url.trim()}>
              Connect to a server
            </Button>
          </>
        )}

        {freed != null ? (
          <span className="dim num" style={{ fontSize: 11 }}>
            Freed {formatBytes(freed)}
          </span>
        ) : null}
        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
