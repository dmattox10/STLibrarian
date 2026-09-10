import { CubeIcon, TagChip } from "./controls/index.jsx";
import { formatBytes, formatCount } from "../lib/format.js";

export default function ModelCard({ model, thumb, selected, onSelect }) {
  return (
    <button
      type="button"
      className="gc-card"
      aria-selected={selected}
      onClick={() => onSelect(model)}
      title={model.path}
    >
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
