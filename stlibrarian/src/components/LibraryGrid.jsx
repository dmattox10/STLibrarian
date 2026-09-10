import { useEffect, useRef } from "react";
import ModelCard from "./ModelCard.jsx";
import { CubeIcon } from "./controls/index.jsx";

/**
 * The library. Arrow keys move the selection the way a console UI would --
 * the grid is a single focus stop and the caret moves inside it.
 */
export default function LibraryGrid({
  models,
  thumbs,
  selected,
  marked,
  onSelect,
  cardSize,
  loading,
  emptyState,
}) {
  const gridRef = useRef(null);

  useEffect(() => {
    const grid = gridRef.current;
    if (!grid) return undefined;

    const onKeyDown = (event) => {
      if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
      if (models.length === 0) return;

      // Read the real column count off the rendered grid rather than guessing,
      // because the track count changes with the window and the size slider.
      const columns = Math.max(
        1,
        getComputedStyle(grid).gridTemplateColumns.split(" ").filter(Boolean).length
      );
      const current = models.findIndex((m) => m.id === selected?.id);
      const from = current === -1 ? 0 : current;
      const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -columns, ArrowDown: columns }[event.key];
      const next = Math.min(models.length - 1, Math.max(0, from + step));

      if (next !== current) {
        event.preventDefault();
        onSelect(models[next], next, { shiftKey: false, metaKey: false, ctrlKey: false });
        grid.children[next]?.scrollIntoView({ block: "nearest" });
      }
    };

    grid.addEventListener("keydown", onKeyDown);
    return () => grid.removeEventListener("keydown", onKeyDown);
  }, [models, selected, onSelect]);

  if (!loading && models.length === 0) {
    return (
      <div className="gc-empty">
        <CubeIcon />
        <h2>{emptyState.title}</h2>
        <p>{emptyState.body}</p>
        {emptyState.action}
      </div>
    );
  }

  return (
    <div
      className="gc-grid"
      ref={gridRef}
      style={{ "--card-size": `${cardSize}px` }}
      role="listbox"
      aria-label="Model library"
      aria-multiselectable="true"
      aria-busy={loading}
    >
      {models.map((model, index) => (
        <ModelCard
          key={model.id}
          model={model}
          index={index}
          thumb={thumbs[model.hash]}
          selected={selected?.id === model.id}
          marked={marked.has(model.id)}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
}
