import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Multi-select over the library grid.
 *
 * The conventions are the ones every file manager uses, because this is the
 * one interaction people already have muscle memory for:
 *
 *   click            focus one model, clear the selection
 *   cmd/ctrl-click   add or remove one model
 *   shift-click      extend from the last anchor
 */
export function useSelection(models, onFocus) {
  const [marked, setMarked] = useState(() => new Set());
  const anchor = useRef(null);

  // When the query changes, ids that are no longer on screen must not stay
  // selected -- a bulk action would otherwise hit models the user cannot see.
  useEffect(() => {
    setMarked((prev) => {
      if (prev.size === 0) return prev;
      const live = new Set(models.map((m) => m.id));
      const next = new Set([...prev].filter((id) => live.has(id)));
      return next.size === prev.size ? prev : next;
    });
    anchor.current = null;
  }, [models]);

  const click = useCallback(
    (model, index, event) => {
      if (event.shiftKey && anchor.current != null) {
        const [from, to] = [anchor.current, index].sort((a, b) => a - b);
        const range = models.slice(from, to + 1).map((m) => m.id);
        setMarked((prev) => new Set([...prev, ...range]));
        return;
      }

      if (event.metaKey || event.ctrlKey) {
        anchor.current = index;
        setMarked((prev) => {
          const next = new Set(prev);
          if (next.has(model.id)) next.delete(model.id);
          else next.add(model.id);
          return next;
        });
        return;
      }

      anchor.current = index;
      setMarked(new Set());
      onFocus(model);
    },
    [models, onFocus]
  );

  const selectAll = useCallback(() => {
    setMarked(new Set(models.map((m) => m.id)));
  }, [models]);

  const clear = useCallback(() => {
    setMarked(new Set());
    anchor.current = null;
  }, []);

  return { marked, click, selectAll, clear };
}
