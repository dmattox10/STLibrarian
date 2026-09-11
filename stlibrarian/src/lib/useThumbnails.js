import { useEffect, useRef, useState } from "react";

/**
 * Resolve a preview for every model on screen.
 *
 * There is no rendering here any more. Previews are rasterised in Rust and
 * cached by content hash, so this only asks for them. That change is what
 * lets a headless server build its own previews -- and it removed the
 * pipeline that used to pull whole 141 MB meshes into the webview and kill
 * the window.
 *
 * Still sequential on the desktop adapter: each miss costs a render, and
 * asking for fifty at once would just queue fifty renders behind each other
 * while making the first one arrive later.
 */
export function useThumbnails(api, models) {
  const [thumbs, setThumbs] = useState({});
  const known = useRef(new Set());
  const runId = useRef(0);

  useEffect(() => {
    const mine = ++runId.current;
    let cancelled = false;

    const pending = models.filter((m) => m.hash && !known.current.has(m.hash));
    if (pending.length === 0) return undefined;

    (async () => {
      for (const model of pending) {
        // A newer query replaced this one; stop rather than fetch previews for
        // models the user has navigated away from.
        if (cancelled || runId.current !== mine) return;
        known.current.add(model.hash);
        try {
          const url = await api.getThumbUrl(model.hash, model);
          if (cancelled || runId.current !== mine) return;
          if (url) setThumbs((prev) => ({ ...prev, [model.hash]: url }));
        } catch {
          // A missing or unreadable file leaves the placeholder. The library
          // keeps working; the inspector reports the real error on click.
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [api, models]);

  return thumbs;
}
