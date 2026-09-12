/* Put believable, truthful content into the throwaway library so the
 * screenshots show a populated app rather than an empty state.
 *
 * Everything here goes in through the real HTTP API, so what the screenshots
 * show is the real UI rendering real database rows.
 *
 * DELIBERATELY NOT SEEDED: reviews. The site renders /api/reviews and the
 * whole design of that feature is that a review comes from somebody who runs
 * the software and is approved by a person first. Inventing testimonials to
 * photograph would be the one thing the feature exists to prevent.
 *
 * Provenance is also left exactly as the scanner inferred it (creator comes
 * from the folder name), rather than being overwritten with invented designer
 * names -- sample geometry should not appear to be somebody else's work.
 */
const BASE = process.env.STLIBRARIAN_URL || "http://127.0.0.1:3999";
const TOKEN = process.argv[2];
if (!TOKEN) throw new Error("usage: node seed.mjs <token>");

async function api(path, method = "GET", body) {
  const res = await fetch(`${BASE}/api${path}`, {
    method,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${await res.text()}`);
  if (res.status === 204) return null;
  const text = await res.text();
  return text ? JSON.parse(text) : null;
}

const models = await api("/models?limit=100");
const byName = Object.fromEntries(models.map((m) => [m.name, m]));
const id = (name) => {
  const m = byName[name];
  if (!m) throw new Error(`no model named ${name}`);
  return m.id;
};

// ---- Tags -----------------------------------------------------------------
// Tags a maker would actually apply: what it is, what it needs, how it prints.
const TAGS = {
  "spur-gear-18t-m2":        ["mechanical", "gear", "petg", "functional"],
  "spur-gear-28t-m2":        ["mechanical", "gear", "petg", "functional"],
  "corner-brace-60mm":       ["mechanical", "bracket", "petg", "needs-supports"],
  "phone-stand-adjustable":  ["desk", "pla", "quick-print"],
  "cable-grommet-28mm":      ["desk", "cable-management", "petg"],
  "knurled-knob-m6":         ["hardware", "knob", "petg", "functional"],
  "hex-standoff-9mm":        ["hardware", "quick-print", "petg"],
  "ribbed-vase-130mm":       ["decorative", "vase-mode", "pla", "gift"],
  "faceted-bowl":            ["decorative", "vase-mode", "pla", "gift"],
  "bud-vase-60mm":           ["decorative", "vase-mode", "pla", "quick-print"],
  "hex-base-26mm":           ["tabletop", "miniature-base", "resin"],
  "round-base-32mm":         ["tabletop", "miniature-base", "resin"],
  "cube-20mm":               ["calibration", "pla"],
  "first-layer-disc-40mm":   ["calibration", "pla", "quick-print"],
};
for (const [name, tags] of Object.entries(TAGS)) {
  await api(`/models/${id(name)}/tags`, "POST", { tags });
}
console.log(`tagged ${Object.keys(TAGS).length} models`);

// ---- Collections ----------------------------------------------------------
const COLLECTIONS = {
  "Printed and proven": ["spur-gear-18t-m2", "phone-stand-adjustable",
                         "knurled-knob-m6", "cube-20mm", "hex-standoff-9mm"],
  "Gifts": ["ribbed-vase-130mm", "faceted-bowl", "bud-vase-60mm"],
  "Printer maintenance": ["cube-20mm", "first-layer-disc-40mm",
                          "cable-grommet-28mm"],
  "Gearbox rebuild": ["spur-gear-18t-m2", "spur-gear-28t-m2",
                      "corner-brace-60mm", "hex-standoff-9mm"],
};
for (const [name, members] of Object.entries(COLLECTIONS)) {
  const c = await api("/collections", "POST", { name, shared: true });
  await api(`/collections/${c.id}/models`, "POST", {
    model_ids: members.map(id),
  });
}
console.log(`created ${Object.keys(COLLECTIONS).length} collections`);

// ---- Print history --------------------------------------------------------
// Printer and material names are real products; the runs are sample records.
const DAY = 86400;
const now = Math.floor(Date.now() / 1000);
const PRINTS = [
  ["cube-20mm", 2, "Prusa MK4", "Prusament PLA Galaxy Black",
   "0.2mm QUALITY, 15% gyroid", "success", "Dialed in after the first layer disc."],
  ["cube-20mm", 9, "Prusa MK4", "Generic PLA",
   "0.2mm QUALITY, 15% gyroid", "success", "Dimensional check: 20.04 / 19.98 / 20.01."],
  ["first-layer-disc-40mm", 10, "Prusa MK4", "Generic PLA",
   "0.2mm, no infill", "success", "Nozzle 0.05 lower than the profile default."],
  ["spur-gear-18t-m2", 4, "Bambu Lab P1S", "Polymaker PETG Black",
   "0.16mm, 40% grid, 4 walls", "success", "Teeth mesh without sanding at 40% infill."],
  ["spur-gear-28t-m2", 4, "Bambu Lab P1S", "Polymaker PETG Black",
   "0.16mm, 40% grid, 4 walls", "partial", "Slight elephant foot; raised the first layer."],
  ["phone-stand-adjustable", 16, "Prusa MK4", "Prusament PLA Jet Black",
   "0.25mm DRAFT, 10% infill", "success", "Draft height is fine for this one."],
  ["knurled-knob-m6", 21, "Bambu Lab P1S", "Polymaker PETG Black",
   "0.12mm, 5 walls", "success", "Knurls come out crisp at 0.12."],
  ["ribbed-vase-130mm", 27, "Prusa MK4", "Prusament PLA Opal Green",
   "Vase mode, 0.3mm, 1.2mm walls", "success", "Nine hours, no babysitting."],
  ["corner-brace-60mm", 33, "Prusa MK4", "Prusament PETG Urban Grey",
   "0.2mm, 30% infill, supports on", "failed", "Warped off the bed. Needs a brim."],
];
for (const [name, daysAgo, printer, material, settings, outcome, notes] of PRINTS) {
  await api(`/models/${id(name)}/prints`, "POST", {
    printed_at: now - daysAgo * DAY,
    printer, material, settings, outcome, notes,
    photo: null, slice_id: null,
  });
}
console.log(`logged ${PRINTS.length} prints`);

const stats = await api("/stats");
console.log("stats:", JSON.stringify(stats));
