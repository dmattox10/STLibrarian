export function formatBytes(bytes) {
  if (!bytes || bytes < 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** i;
  return `${value >= 100 || i === 0 ? Math.round(value) : value.toFixed(1)} ${units[i]}`;
}

export function formatCount(n) {
  if (n == null) return "0";
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** STLs are unitless by spec; millimetres is the universal convention in print. */
export function formatDims(model) {
  const round = (v) => (v >= 100 ? Math.round(v) : Math.round(v * 10) / 10);
  if (!model?.dim_x && !model?.dim_y && !model?.dim_z) return "unknown";
  return `${round(model.dim_x)} x ${round(model.dim_y)} x ${round(model.dim_z)} mm`;
}

/** Does it fit the bed? The question every print library exists to answer. */
export function fitsBed(model, bed = { x: 220, y: 220, z: 250 }) {
  if (!model) return null;
  const dims = [model.dim_x, model.dim_y, model.dim_z];
  if (dims.every((d) => !d)) return null;
  // Only the footprint may be rotated; Z is Z.
  const [a, b] = [dims[0], dims[1]].sort((m, n) => m - n);
  const [bx, by] = [bed.x, bed.y].sort((m, n) => m - n);
  return a <= bx && b <= by && dims[2] <= bed.z;
}

export function formatDate(unixSeconds) {
  if (!unixSeconds) return "unknown";
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

const AURORA = ["--nord14", "--nord8", "--nord13", "--nord15", "--nord12", "--nord7", "--nord9"];

/**
 * A tag always gets the same colour, so the eye learns the library.
 * `untagged` is deliberately grey -- it is the absence of a category, and
 * colouring it would make unsorted models look sorted.
 */
export function tagColor(name) {
  if (!name || name.toLowerCase() === "untagged") return "var(--nord3)";
  let hash = 0;
  for (let i = 0; i < name.length; i += 1) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  }
  return `var(${AURORA[hash % AURORA.length]})`;
}

/** Resin and filament densities, g/cm3. Close enough for a shopping decision. */
const DENSITY = { resin: 1.1, pla: 1.24 };

/**
 * Whether the volume means anything.
 *
 * It comes from summing signed tetrahedra, which is exact for a closed mesh
 * and meaningless for an open one -- and sculpted models frequently are not
 * closed. This used to guess from the volume-to-bounding-box ratio, which
 * cannot tell a genuinely thin model from a mesh full of holes. The backend
 * now counts edge parity instead, so this is a fact rather than a hunch.
 */
export function volumeLooksReliable(model) {
  return Boolean(model?.volume > 0 && model?.watertight);
}

export function formatVolume(mm3) {
  if (!mm3 || mm3 <= 0) return null;
  const cm3 = mm3 / 1000;
  return cm3 >= 100 ? `${Math.round(cm3)} cm3` : `${cm3.toFixed(1)} cm3`;
}

/**
 * Grams of material for a solid print.
 *
 * Deliberately an over-estimate: real prints are hollowed or infilled, so this
 * is the ceiling rather than a quote. A ceiling is the useful number when you
 * are deciding whether you have enough resin in the bottle.
 */
export function formatWeight(mm3, material = "resin") {
  if (!mm3 || mm3 <= 0) return null;
  const grams = (mm3 / 1000) * (DENSITY[material] ?? DENSITY.resin);
  return grams >= 100 ? `${Math.round(grams)} g` : `${grams.toFixed(1)} g`;
}
