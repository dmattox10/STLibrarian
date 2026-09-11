import * as THREE from "three";

/** Triangle budgets. A 320px card cannot show more than the first of these. */
export const THUMB_MAX_TRIANGLES = 60_000;
export const VIEWER_MAX_TRIANGLES = 250_000;

/**
 * Build geometry from the decimated position buffer the backend sends.
 *
 * Note what is deliberately absent: any STL parsing. The front end used to
 * pull whole files across and hand them to STLLoader, which meant a 141 MB /
 * 3M triangle model became roughly 355 MB of webview memory -- file, plus
 * positions, plus normals -- and killed the window. The backend streams and
 * decimates instead, so what arrives here is a few megabytes of floats.
 */
export function geometryFromPositions(buffer) {
  const positions = new Float32Array(buffer);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  // Non-indexed geometry, so this produces one normal per face -- the flat
  // faceting an STL actually describes.
  geometry.computeVertexNormals();
  // Print STLs are authored Z-up, sitting on the bed. three.js is Y-up, so
  // without this every model in the library lies on its back.
  geometry.rotateX(-Math.PI / 2);
  geometry.center();
  geometry.computeBoundingSphere();
  return geometry;
}

export function makeMaterial() {
  return new THREE.MeshStandardMaterial({
    // Light, near-neutral: a printed part in grey resin. The old frost-teal
    // at 0x8fbcbb was a mid-tone, and against the dark stage it read as an
    // almost black silhouette once the wireframe was off.
    color: 0xc4d2d8,
    metalness: 0.0,
    roughness: 0.62,
    flatShading: true,
  });
}

/**
 * Wireframe.
 *
 * Unlit on purpose. Setting `wireframe: true` on the lit surface material
 * shades each line by its face normal, so every edge pointing away from the
 * key light renders almost black -- the mesh looks like it is drawn in dark
 * grey on a dark background. An unlit material gives every edge the same
 * brightness, which is the whole point of looking at a wireframe.
 */
export function makeWireMaterial() {
  return new THREE.MeshBasicMaterial({
    color: 0x88c0d0,
    wireframe: true,
    transparent: true,
    opacity: 0.85,
  });
}

/**
 * Point cloud. Every vertex of the decimated mesh, drawn unconnected.
 *
 * Keeps the look the clustering accidentally produced along the way: with no
 * surface to hide behind, interior structure and wall thickness show through.
 */
export function makePointsMaterial(radius) {
  return new THREE.PointsMaterial({
    color: 0x88c0d0,
    // Scaled to the model so a 10mm part and a 300mm one look alike.
    size: Math.max(radius * 0.008, 0.0001),
    sizeAttenuation: true,
    transparent: true,
    opacity: 0.9,
  });
}

export function lightScene(scene) {
  // Three-point, deliberately generous. A flat-shaded mesh has no specular
  // help, so under-lighting it turns every facet the same dark tone.
  scene.add(new THREE.AmbientLight(0xd8dee9, 0.55));
  scene.add(new THREE.HemisphereLight(0xeceff4, 0x434c5e, 1.5));

  const key = new THREE.DirectionalLight(0xffffff, 2.1);
  key.position.set(4, 7, 6);
  scene.add(key);

  const fill = new THREE.DirectionalLight(0x8fbcbb, 0.9);
  fill.position.set(-5, 1, 4);
  scene.add(fill);

  // Frost-blue rim from behind, so silhouettes separate from the dark ground.
  const rim = new THREE.DirectionalLight(0x5e81ac, 1.2);
  rim.position.set(-6, 2, -5);
  scene.add(rim);
}

/** Pull the camera back far enough to hold `radius` in frame. */
export function frameCamera(camera, radius, { azimuth = 0.9, elevation = 0.55, pad = 1.25 } = {}) {
  const safeRadius = radius > 0 ? radius : 1;
  const distance = (safeRadius / Math.sin((camera.fov * Math.PI) / 360)) * pad;
  camera.position.set(
    distance * Math.cos(elevation) * Math.sin(azimuth),
    distance * Math.sin(elevation),
    distance * Math.cos(elevation) * Math.cos(azimuth)
  );
  camera.near = distance / 100;
  camera.far = distance * 12;
  camera.lookAt(0, 0, 0);
  camera.updateProjectionMatrix();
}

/* ------------------------------------------------------------ thumbnails -- */

let forge = null;

/**
 * One offscreen renderer, reused for every preview.
 *
 * Browsers cap live WebGL contexts at around sixteen and silently drop the
 * oldest past that. Making a renderer per thumbnail would kill the interactive
 * viewer partway through a library scan.
 */
function getForge(size) {
  if (forge && forge.size === size) return forge;
  if (forge) {
    forge.renderer.dispose();
    forge = null;
  }
  try {
    const renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      // Required for toDataURL to see anything on most drivers.
      preserveDrawingBuffer: true,
    });
    renderer.setSize(size, size, false);
    renderer.setPixelRatio(1);

    const scene = new THREE.Scene();
    lightScene(scene);
    const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 1000);
    forge = { renderer, scene, camera, size };
    return forge;
  } catch {
    // No WebGL (software rendering disabled, headless, a locked-down browser).
    // Previews are a nicety; the library still works without them.
    return null;
  }
}

/**
 * Render STL bytes to a PNG data URL, or null if the GPU is unavailable.
 */
export function renderThumbnail(buffer, size = 512) {
  const f = getForge(size);
  if (!f) return null;

  let geometry;
  let mesh;
  try {
    geometry = geometryFromPositions(buffer);
    if (!geometry.boundingSphere || geometry.boundingSphere.radius === 0) return null;

    const material = makeMaterial();
    mesh = new THREE.Mesh(geometry, material);
    f.scene.add(mesh);
    frameCamera(f.camera, geometry.boundingSphere.radius, { pad: 1.2 });
    f.renderer.render(f.scene, f.camera);
    return f.renderer.domElement.toDataURL("image/png");
  } catch {
    return null;
  } finally {
    // Geometry and materials are not garbage collected -- a scan of ten
    // thousand models would leak every one of them onto the GPU.
    if (mesh) {
      f.scene.remove(mesh);
      mesh.material.dispose();
    }
    if (geometry) geometry.dispose();
  }
}

export function disposeForge() {
  if (forge) {
    forge.renderer.dispose();
    forge = null;
  }
}
