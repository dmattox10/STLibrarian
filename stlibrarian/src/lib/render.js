import * as THREE from "three";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

const loader = new STLLoader();

/**
 * Turn raw STL bytes into a centred, upright, flat-shaded geometry.
 */
export function parseStl(buffer) {
  const geometry = loader.parse(buffer);
  // An STL is a soup of independent triangles with per-facet normals that are
  // frequently wrong or absent. Recomputing on non-indexed geometry gives one
  // normal per face, which is the faceting the format actually describes.
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
    color: 0x8fbcbb,
    metalness: 0.12,
    roughness: 0.58,
    flatShading: true,
  });
}

export function lightScene(scene) {
  scene.add(new THREE.HemisphereLight(0xd8dee9, 0x2e3440, 1.15));

  const key = new THREE.DirectionalLight(0xeceff4, 1.6);
  key.position.set(4, 7, 6);
  scene.add(key);

  // Frost-blue rim from behind, so silhouettes separate from the dark ground.
  const rim = new THREE.DirectionalLight(0x5e81ac, 1.1);
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
    geometry = parseStl(buffer);
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
