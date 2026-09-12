/* Generate a small sample library of real binary STL meshes.
 *
 * These exist so the screenshots on stlibrarian.com show the actual UI with
 * actual geometry in it -- real meshes the real scanner measures and the real
 * Rust previewer renders. Nothing here is drawn or mocked; the only synthetic
 * part is that the models are generated rather than downloaded, which keeps
 * the licensing of everything on the marketing site unambiguous.
 */
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const T = [];                                   // accumulating triangle list
const tri = (a, b, c) => T.push([a, b, c]);
const add = (v, w) => [v[0] + w[0], v[1] + w[1], v[2] + w[2]];

/** A closed, star-shaped 2D polygon extruded along +Z, with capped ends. */
function extrude(poly, z0, z1, offset = [0, 0, 0]) {
  const n = poly.length;
  // Caps fan from the centroid. Valid because every polygon passed here is
  // star-shaped about it -- gears, knurled knobs, rings, discs.
  const cx = poly.reduce((s, p) => s + p[0], 0) / n;
  const cy = poly.reduce((s, p) => s + p[1], 0) / n;
  for (let i = 0; i < n; i++) {
    const [x1, y1] = poly[i];
    const [x2, y2] = poly[(i + 1) % n];
    const a = add([x1, y1, z0], offset), b = add([x2, y2, z0], offset);
    const c = add([x2, y2, z1], offset), d = add([x1, y1, z1], offset);
    tri(a, b, c); tri(a, c, d);                         // side wall
    tri(add([cx, cy, z0], offset), b, a);               // bottom (wound inward)
    tri(add([cx, cy, z1], offset), d, c);               // top
  }
}

/** Revolve a profile of [radius, z] pairs about the Z axis. */
function revolve(profile, steps = 64, offset = [0, 0, 0]) {
  for (let i = 0; i < steps; i++) {
    const t0 = (i / steps) * Math.PI * 2;
    const t1 = ((i + 1) / steps) * Math.PI * 2;
    for (let j = 0; j < profile.length - 1; j++) {
      const [r0, z0] = profile[j];
      const [r1, z1] = profile[j + 1];
      const p = (r, z, t) => add([r * Math.cos(t), r * Math.sin(t), z], offset);
      tri(p(r0, z0, t0), p(r1, z1, t0), p(r1, z1, t1));
      tri(p(r0, z0, t0), p(r1, z1, t1), p(r0, z0, t1));
    }
  }
}

function box(x0, y0, z0, x1, y1, z1) {
  const v = [[x0,y0,z0],[x1,y0,z0],[x1,y1,z0],[x0,y1,z0],
             [x0,y0,z1],[x1,y0,z1],[x1,y1,z1],[x0,y1,z1]];
  const f = [[0,3,2],[0,2,1],[4,5,6],[4,6,7],[0,1,5],[0,5,4],
             [1,2,6],[1,6,5],[2,3,7],[2,7,6],[3,0,4],[3,4,7]];
  for (const [a,b,c] of f) tri(v[a], v[b], v[c]);
}

/** Circle sampled as a polygon, with trapezoidal teeth or knurls cut into it. */
function toothedCircle(r, teeth, depth, steps = 8) {
  const pts = [];
  for (let i = 0; i < teeth; i++) {
    for (let s = 0; s < steps; s++) {
      const t = ((i + s / steps) / teeth) * Math.PI * 2;
      // Flat crest, straight flanks, flat root -- reads as a tooth at a glance.
      const phase = (s / steps) % 1;
      const rr = phase < 0.3 ? r
               : phase < 0.5 ? r - depth * (phase - 0.3) / 0.2
               : phase < 0.8 ? r - depth
               : r - depth * (1 - (phase - 0.8) / 0.2);
      pts.push([rr * Math.cos(t), rr * Math.sin(t)]);
    }
  }
  return pts;
}

function circle(r, steps = 48) {
  return Array.from({ length: steps }, (_, i) => {
    const t = (i / steps) * Math.PI * 2;
    return [r * Math.cos(t), r * Math.sin(t)];
  });
}

function write(path, name) {
  mkdirSync(dirname(path), { recursive: true });
  const buf = Buffer.alloc(84 + T.length * 50);
  buf.write(`STLibrarian sample: ${name}`.slice(0, 79), 0, "ascii");
  buf.writeUInt32LE(T.length, 80);
  let o = 84;
  for (const [a, b, c] of T) {
    // Facet normal from the winding, so slicers and the app agree on outside.
    const u = [b[0]-a[0], b[1]-a[1], b[2]-a[2]];
    const v = [c[0]-a[0], c[1]-a[1], c[2]-a[2]];
    const nx = u[1]*v[2]-u[2]*v[1];
    const ny = u[2]*v[0]-u[0]*v[2];
    const nz = u[0]*v[1]-u[1]*v[0];
    const len = Math.hypot(nx, ny, nz) || 1;
    for (const f of [nx/len, ny/len, nz/len, ...a, ...b, ...c]) {
      buf.writeFloatLE(f, o); o += 4;
    }
    buf.writeUInt16LE(0, o); o += 2;
  }
  writeFileSync(path, buf);
  console.log(`${String(T.length).padStart(6)} tris  ${path}`);
  T.length = 0;
}

const ROOT = process.argv[2] || "./library";

// ---- Functional prints -----------------------------------------------------
extrude(toothedCircle(22, 18, 4), 0, 8);                       // spur gear body
extrude(circle(4), -1, 9);                                     // bore
write(`${ROOT}/Functional/Gears/spur-gear-18t-m2.stl`, "Spur gear, 18 tooth");

extrude(toothedCircle(34, 28, 5), 0, 8);
extrude(circle(6), -1, 9);
write(`${ROOT}/Functional/Gears/spur-gear-28t-m2.stl`, "Spur gear, 28 tooth");

box(0, 0, 0, 60, 5, 40);  box(0, 0, 0, 5, 45, 40);             // L bracket
box(12, -1, 8, 20, 6, 16); box(38, -1, 8, 46, 6, 16);          // lightening pockets
write(`${ROOT}/Functional/Brackets/corner-brace-60mm.stl`, "Corner brace 60mm");

box(0, 0, 0, 90, 70, 4);                                       // phone stand
box(0, 0, 0, 90, 4, 55);
box(0, 60, 0, 90, 70, 14);
write(`${ROOT}/Functional/Desk/phone-stand-adjustable.stl`, "Phone stand");

extrude(circle(14), 0, 6); extrude(circle(9), -1, 7);          // cable grommet
write(`${ROOT}/Functional/Desk/cable-grommet-28mm.stl`, "Cable grommet 28mm");

extrude(toothedCircle(16, 30, 1.6), 0, 18);                    // knurled knob
extrude(circle(3), -1, 19);
write(`${ROOT}/Functional/Hardware/knurled-knob-m6.stl`, "Knurled knob M6");

extrude(circle(11, 6), 0, 9);                                  // hex standoff
extrude(circle(2.5), -1, 10);
write(`${ROOT}/Functional/Hardware/hex-standoff-9mm.stl`, "Hex standoff 9mm");

// ---- Decorative ------------------------------------------------------------
revolve([[0,0],[34,0],[34,3],[30,3],[36,40],[30,95],[38,130],[34,132],[30,130],
         [26,95],[26,40],[30,6],[0,6]], 96);
write(`${ROOT}/Decorative/Vases/ribbed-vase-130mm.stl`, "Ribbed vase 130mm");

revolve([[0,0],[45,0],[45,4],[42,4],[48,22],[52,44],[50,46],[44,44],[40,22],
         [38,6],[0,6]], 96);
write(`${ROOT}/Decorative/Bowls/faceted-bowl.stl`, "Faceted bowl");

revolve([[0,0],[18,0],[18,2],[15,2],[22,30],[16,58],[20,62],[14,60],[12,30],
         [14,4],[0,4]], 64);
write(`${ROOT}/Decorative/Vases/bud-vase-60mm.stl`, "Bud vase 60mm");

// ---- Tabletop --------------------------------------------------------------
extrude(circle(13, 6), 0, 3);                                  // hex base
extrude(circle(11, 6), 3, 4);
box(-2, -2, 4, 2, 2, 26);                                      // post
revolve([[0,26],[6,26],[7,32],[4,38],[0,39]], 32);             // rounded top
write(`${ROOT}/Tabletop/Bases/hex-base-26mm.stl`, "Hex base 26mm");

extrude(circle(16, 48), 0, 2.5);                               // round base
extrude(circle(14, 48), 2.5, 3.5);
write(`${ROOT}/Tabletop/Bases/round-base-32mm.stl`, "Round base 32mm");

// ---- Calibration -----------------------------------------------------------
box(0, 0, 0, 20, 20, 20);
write(`${ROOT}/Calibration/cube-20mm.stl`, "Calibration cube 20mm");

extrude(circle(20, 64), 0, 0.6);                               // first layer disc
write(`${ROOT}/Calibration/first-layer-disc-40mm.stl`, "First layer disc 40mm");
