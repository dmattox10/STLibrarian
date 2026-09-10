import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { frameCamera, lightScene, makeMaterial, parseStl } from "../lib/render.js";
import { Button, Toggle, RefreshIcon } from "./controls/index.jsx";

/**
 * Interactive preview for one model.
 *
 * Owns exactly one WebGL context for the life of the component and swaps the
 * mesh inside it when the selected model changes, rather than tearing the
 * renderer down and rebuilding it on every click.
 */
export default function StlViewer({ api, model }) {
  const hostRef = useRef(null);
  const sceneRef = useRef(null);
  const [status, setStatus] = useState("idle");
  const [error, setError] = useState(null);
  const [spin, setSpin] = useState(true);
  const [wireframe, setWireframe] = useState(false);

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

    const state = { renderer, scene, camera, controls, mesh: null, spin: true, radius: 1 };
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
      if (state.spin && state.mesh) state.mesh.rotation.y += 0.006;
      controls.update();
      renderer.render(scene, camera);
    };
    tick();

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      controls.dispose();
      if (state.mesh) {
        scene.remove(state.mesh);
        state.mesh.geometry.dispose();
        state.mesh.material.dispose();
      }
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

    api
      .readModel(model.id)
      .then((buffer) => {
        if (cancelled || !sceneRef.current) return;
        const geometry = parseStl(buffer);
        const mesh = new THREE.Mesh(geometry, makeMaterial());
        mesh.material.wireframe = wireframe;

        if (state.mesh) {
          state.scene.remove(state.mesh);
          state.mesh.geometry.dispose();
          state.mesh.material.dispose();
        }
        state.mesh = mesh;
        state.radius = geometry.boundingSphere?.radius || 1;
        state.scene.add(mesh);
        frameCamera(state.camera, state.radius);
        state.controls.target.set(0, 0, 0);
        state.controls.update();
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
    // `wireframe` is applied separately below; re-reading the file when a
    // checkbox flips would be absurd for a 300 MB model.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, model?.id]);

  useEffect(() => {
    const state = sceneRef.current;
    if (state?.mesh) state.mesh.material.wireframe = wireframe;
  }, [wireframe]);

  useEffect(() => {
    if (sceneRef.current) sceneRef.current.spin = spin;
  }, [spin]);

  const resetView = () => {
    const state = sceneRef.current;
    if (!state) return;
    frameCamera(state.camera, state.radius);
    state.controls.target.set(0, 0, 0);
    state.controls.update();
    if (state.mesh) state.mesh.rotation.y = 0;
  };

  return (
    <div className="viewer">
      <div className="viewer__stage" ref={hostRef}>
        {status === "loading" ? <span className="viewer__status">Loading mesh…</span> : null}
        {error ? <span className="viewer__status viewer__status--error">{error}</span> : null}
      </div>
      <div className="viewer__hud">
        <Toggle checked={spin} onChange={setSpin}>
          Spin
        </Toggle>
        <Toggle checked={wireframe} onChange={setWireframe}>
          Wire
        </Toggle>
        <div style={{ flex: 1 }} />
        <Button size="sm" onClick={resetView} title="Reset view">
          <RefreshIcon />
          Reset
        </Button>
      </div>
    </div>
  );
}
