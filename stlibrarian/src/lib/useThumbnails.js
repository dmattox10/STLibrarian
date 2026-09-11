import { useEffect, useRef, useState } from "react";
import { renderThumbnail, THUMB_MAX_TRIANGLES } from "./render.js";

/**
 * 320px rather than 512.
 *
 * On the desktop adapter a preview comes back as a base64 data URL held in JS
 * memory. At 512px that is roughly 200 KB per model, and a 500-model page would
 * cost 100 MB of strings. 320 is still crisp on a grid card at 4x3.
 */
const THUMB_SIZE = 320;

/**
 * Resolve a preview for every model on screen, rendering the missing ones.
 *
 * Strictly sequential: a scan can queue thousands of models, and each render
 * means reading an entire STL off disk. Doing those in parallel would saturate
 * the disk and stall the UI for no gain -- there is only one GPU context.
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
        // A new query replaced this one; stop rather than render previews for
        // models the user has already navigated away from.
        if (cancelled || runId.current !== mine) return;
        known.current.add(model.hash);

        try {
          const existing = await api.getThumbUrl(model.hash, model);
          if (existing) {
            if (cancelled) return;
            setThumbs((prev) => ({ ...prev, [model.hash]: existing }));
            continue;
          }

          // Decimated, never the whole file.
          const buffer = await api.readPreview(model.id, THUMB_MAX_TRIANGLES);
          if (cancelled || runId.current !== mine) return;

          const dataUrl = renderThumbnail(buffer, THUMB_SIZE);
          if (!dataUrl) {
            // No WebGL. Stop trying -- every subsequent model would fail the
            // same way and each attempt costs a full file read.
            return;
          }

          setThumbs((prev) => ({ ...prev, [model.hash]: dataUrl }));
          await api.putThumb(model.hash, dataUrl).catch(() => {
            // The preview is on screen; failing to cache it is not worth
            // interrupting the user over. It will be re-rendered next time.
          });
        } catch {
          // A missing or unreadable file shows the placeholder. The library
          // keeps working; the Inspector reports the real error on click.
        }

        // Yield so the grid stays responsive between renders.
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [api, models]);

  return thumbs;
}
