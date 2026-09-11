import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import {
  frameCamera,
  geometryFromPositions,
  lightScene,
  makeMaterial,
  makePointsMaterial,
  makeWireMaterial,
  VIEWER_MAX_TRIANGLES,
} from "../lib/render.js";
import { Button, ExpandIcon, RefreshIcon, Segmented, Toggle } from "./controls/index.jsx";

const MODES = [
  { value: "solid", label: "Solid" },
  { value: "wire", label: "Wire" },
  { value: "points", label: "Points" },
];

/**
 * Interactive preview for one model.
 *
 * Owns exactly one WebGL context for the life of the component and swaps what
 * is inside it when the model or the view mode changes, rather than tearing
 * the renderer down and rebuilding it.
 */
export default function StlViewer({ api, model }) {
  const hostRef = useRef(null);
  const sceneRef = useRef(null);
  const [status, setStatus] = useState("idle");
  const [error, setError] = useState(null);
  const [spin, setSpin] = useState(true);
  const [mode, setMode] = useState("solid");
  const [expanded, setExpanded] = useState(false);

  // --- one-time renderer setup -------------------------------------------
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return undefined;

    let renderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    } catch {
      setError("3D preview needs WebGL, which this system did not provide.");
      return undefined;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(host.clientWidth || 300, host.clientHeight || 220, false);
    host.appendChild(renderer.domElement);
    renderer.domElement.style.display = "block";
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.height = "100%";

    const scene = new THREE.Scene();
    lightScene(scene);
    const camera = new THREE.PerspectiveCamera(38, 1, 0.1, 1000);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;
    // Panning matters once you are zoomed in on one feature; Reset recovers
    // the framing if you lose the model off screen.
    controls.enablePan = true;
    controls.screenSpacePanning = true;

    const state = {
      renderer,
      scene,
      camera,
      controls,
      object: null,
      geometry: null,
      spin: true,
      radius: 1,
    };
    sceneRef.current = state;

    const resize = () => {
      const w = host.clientWidth;
      const h = host.clientHeight;
      if (!w || !h) return;
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(host);

    let frame;
    const tick = () => {
      frame = requestAnimationFrame(tick);
      // Spin the model, not the camera, so the user's orbit is preserved.
      if (state.spin && state.object) state.object.rotation.y += 0.006;
      controls.update();
      renderer.render(scene, camera);
    };
    tick();

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      controls.dispose();
      if (state.object) {
        scene.remove(state.object);
        state.object.material.dispose();
      }
      if (state.geometry) state.geometry.dispose();
      renderer.dispose();
      renderer.domElement.remove();
      sceneRef.current = null;
    };
  }, []);

  // --- load the selected model -------------------------------------------
  useEffect(() => {
    const state = sceneRef.current;
    if (!state || !model) return undefined;

    let cancelled = false;
    setStatus("loading");
    setError(null);

    // A higher budget than a thumbnail gets, but still a budget: opening one
    // model should not be able to exhaust the webview.
    api
      .readPreview(model.id, VIEWER_MAX_TRIANGLES)
      .then((buffer) => {
        if (cancelled || !sceneRef.current) return;
        const geometry = geometryFromPositions(buffer);

        if (state.object) {
          state.scene.remove(state.object);
          state.object.material.dispose();
        }
        if (state.geometry) state.geometry.dispose();

        state.geometry = geometry;
        state.radius = geometry.boundingSphere?.radius || 1;
        state.object = null; // rebuilt by the mode effect below
        setStatus("ready");
      })
      .catch((e) => {
        if (cancelled) return;
        setError(e?.message || "Could not read this model.");
        setStatus("error");
      });

    return () => {
      cancelled = true;
    };
  }, [api, model?.id]);

  // --- build the object for the current mode ------------------------------
  useEffect(() => {
    const state = sceneRef.current;
    if (!state?.geometry) return;

    const spin = state.object?.rotation.y ?? 0;
    if (state.object) {
      state.scene.remove(state.object);
      state.object.material.dispose();
    }

    let object;
    if (mode === "points") {
      object = new THREE.Points(state.geometry, makePointsMaterial(state.radius));
    } else if (mode === "wire") {
      object = new THREE.Mesh(state.geometry, makeWireMaterial());
    } else {
      object = new THREE.Mesh(state.geometry, makeMaterial());
    }
    // Keep whatever rotation the spin had reached, so switching mode does not
    // snap the model back to its start.
    object.rotation.y = spin;
    state.object = object;
    state.scene.add(object);

    if (!state.framed) {
      frameCamera(state.camera, state.radius);
      state.controls.target.set(0, 0, 0);
      state.controls.update();
      state.framed = true;
    }
  }, [mode, status]);

  useEffect(() => {
    if (sceneRef.current) sceneRef.current.spin = spin;
  }, [spin]);

  // Re-frame when a different model loads.
  useEffect(() => {
    if (sceneRef.current) sceneRef.current.framed = false;
  }, [model?.id]);

  const resetView = () => {
    const state = sceneRef.current;
    if (!state) return;
    frameCamera(state.camera, state.radius);
    state.controls.target.set(0, 0, 0);
    state.controls.update();
    if (state.object) state.object.rotation.y = 0;
  };

  return (
    <div className={`viewer${expanded ? " viewer--expanded" : ""}`}>
      <div className="viewer__stage" ref={hostRef}>
        {status === "loading" ? <span className="viewer__status">Loading mesh…</span> : null}
        {error ? <span className="viewer__status viewer__status--error">{error}</span> : null}
      </div>
      <div className="viewer__hud">
        <Segmented value={mode} onChange={setMode} options={MODES} label="View mode" />
        <Toggle checked={spin} onChange={setSpin}>
          Spin
        </Toggle>
        <div style={{ flex: 1 }} />
        <Button size="sm" icon onClick={resetView} title="Reset view" aria-label="Reset view">
          <RefreshIcon />
        </Button>
        <Button
          size="sm"
          icon
          onClick={() => setExpanded((v) => !v)}
          title={expanded ? "Shrink" : "Expand"}
          aria-label={expanded ? "Shrink viewer" : "Expand viewer"}
          aria-pressed={expanded}
        >
          <ExpandIcon />
        </Button>
      </div>
    </div>
  );
}
