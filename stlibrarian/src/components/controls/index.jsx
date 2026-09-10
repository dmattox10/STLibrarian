/**
 * The control set. Every one of these is a plain DOM element with the
 * `gc-` classes from theme/controls.css -- no component library, because the
 * look is the point and a library would have to be fought the whole way.
 */
import { tagColor } from "../../lib/format.js";

export function Button({ variant, size, icon, children, ...rest }) {
  const classes = ["gc-btn"];
  if (variant) classes.push(`gc-btn--${variant}`);
  if (size) classes.push(`gc-btn--${size}`);
  if (icon) classes.push("gc-btn--icon");
  return (
    <button type="button" className={classes.join(" ")} {...rest}>
      {children}
    </button>
  );
}

export function Segmented({ value, onChange, options, label }) {
  return (
    <div className="gc-seg" role="group" aria-label={label}>
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          className="gc-seg__opt"
          aria-pressed={value === opt.value}
          onClick={() => onChange(opt.value)}
        >
          {opt.label}
        </button>
      ))}
    </div>
  );
}

export function Toggle({ checked, onChange, children }) {
  return (
    <label className="gc-toggle">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="gc-toggle__track">
        <span className="gc-toggle__knob" />
      </span>
      <span className="gc-toggle__label">{children}</span>
    </label>
  );
}

export function Field({ icon, children, style }) {
  return (
    <div className="gc-field" style={style}>
      {icon ? (
        <span className="gc-field__icon" aria-hidden="true">
          {icon}
        </span>
      ) : null}
      {children}
    </div>
  );
}

export function SearchField({ value, onChange, placeholder = "Search", style }) {
  return (
    <Field icon={<SearchIcon />} style={style}>
      <input
        type="search"
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        aria-label={placeholder}
      />
      {value ? (
        <button
          type="button"
          className="gc-chip__x"
          onClick={() => onChange("")}
          aria-label="Clear search"
        >
          &times;
        </button>
      ) : null}
    </Field>
  );
}

export function Panel({ title, actions, children }) {
  return (
    <section className="gc-panel">
      {title ? (
        <header className="gc-panel__head">
          <h2 className="gc-panel__title">{title}</h2>
          <div style={{ flex: 1 }} />
          {actions}
        </header>
      ) : null}
      <div className="gc-panel__body">{children}</div>
    </section>
  );
}

export function TagChip({ name, count, pressed, onClick, onRemove }) {
  return (
    <span className="gc-chip" style={{ color: tagColor(name) }} aria-pressed={pressed}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onClick={onClick}
      onKeyDown={(e) => {
        if (onClick && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <span>{name}</span>
      {count != null ? <span className="gc-chip__count">{count}</span> : null}
      {onRemove ? (
        <button
          type="button"
          className="gc-chip__x"
          aria-label={`Remove tag ${name}`}
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
        >
          &times;
        </button>
      ) : null}
    </span>
  );
}

export function Meter({ value, busy, label }) {
  return (
    <div
      className={`gc-meter${busy ? " gc-meter--busy" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuenow={busy ? undefined : Math.round((value ?? 0) * 100)}
    >
      <div className="gc-meter__fill" style={busy ? undefined : { width: `${(value ?? 0) * 100}%` }} />
    </div>
  );
}

export function Alert({ children }) {
  if (!children) return null;
  return <div className="gc-alert">{children}</div>;
}

/* ------------------------------------------------------------------ icons -- */
/* Inline so the app ships no icon font and no extra network request. */

const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
};

export function SearchIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" {...stroke} />
      <path d="M10.5 10.5 14 14" {...stroke} />
    </svg>
  );
}

export function FolderIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M1.5 3.5h4l1.5 2h7.5v7h-13z" {...stroke} />
    </svg>
  );
}

export function CubeIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M8 1.5 14 5v6l-6 3.5L2 11V5z" {...stroke} />
      <path d="M2 5l6 3.5L14 5M8 8.5V14.5" {...stroke} />
    </svg>
  );
}

export function TrashIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4" {...stroke} />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M4 4l8 8M12 4l-8 8" {...stroke} />
    </svg>
  );
}

export function RefreshIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M13.5 8a5.5 5.5 0 1 1-1.7-4" {...stroke} />
      <path d="M13.5 1.5V4.5h-3" {...stroke} />
    </svg>
  );
}
