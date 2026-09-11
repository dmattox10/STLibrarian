import { CubeIcon, TagChip } from "./controls/index.jsx";
import { formatBytes, formatCount } from "../lib/format.js";

export default function ModelCard({ model, thumb, selected, marked, index, onSelect }) {
  return (
    <button
      type="button"
      className={`gc-card${marked ? " gc-card--marked" : ""}`}
      aria-selected={marked || selected}
      onClick={(event) => onSelect(model, index, event)}
      title={model.path}
    >
      {marked ? (
        <span className="gc-card__check" aria-hidden="true">
          &#10003;
        </span>
      ) : null}
      <span className="gc-card__thumb">
        {thumb ? (
          // Decorative: the name directly below says what this is, so a
          // screen reader announcing the filename twice helps nobody.
          <img src={thumb} alt="" loading="lazy" />
        ) : (
          <span className="gc-card__pending">
            <CubeIcon />
          </span>
        )}
      </span>
      <span className="gc-card__body">
        <span className="gc-card__name">{model.name || "(unnamed)"}</span>
        <span className="gc-card__meta">
          <span>{formatBytes(model.size)}</span>
          <span>{formatCount(model.triangles)} tri</span>
          {model.variant === "supported" ? (
            <span className="gc-card__variant" title="Presupported">
              S
            </span>
          ) : null}
          {model.print_count > 0 ? (
            <span className="gc-card__printed" title={`Printed ${model.print_count}x`}>
              &#10003; {model.print_count > 1 ? model.print_count : ""}
            </span>
          ) : null}
        </span>
        <span className="gc-card__tags">
          {model.tags.slice(0, 3).map((tag) => (
            <TagChip key={tag} name={tag} />
          ))}
        </span>
      </span>
    </button>
  );
}
