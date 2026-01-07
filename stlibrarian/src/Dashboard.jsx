import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export default function Dashboard() {
  const [models, setModels] = useState([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    invoke("models").then(setModels);

    const unlistenModel = listen("model-indexed", (event) => {
      setModels((prev) => [...prev, event.payload]);
    });

    const unlistenComplete = listen("scan-complete", () => {
      setLoading(false); // stop spinner
    });

    return () => {
      unlistenModel.then((f) => f());
      unlistenComplete.then((f) => f());
    };
  }, []);

  async function pickFolderAndScan() {
    const folder = await open({ directory: true, multiple: false });
    if (!folder) return;

    setLoading(true);
    await invoke("scan", { path: folder });
    setLoading(false);
  }

  return (
    <div style={{ padding: 20 }}>
      <h1>Model Library</h1>

      <button onClick={pickFolderAndScan}>Scan Folder</button>

      {loading && <p>Scanning…</p>}

      {models.length === 0 && !loading && (
        <p>No models yet. Click “Scan Folder”.</p>
      )}

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, 1fr)",
          gap: 10,
          marginTop: 20,
        }}
      >
        {models.map((m) => (
          <div key={m.id} style={{ padding: 10, border: "1px solid #ccc" }}>
            <div>{m.path.split("/").pop()}</div>
            <div style={{ fontSize: 12, opacity: 0.6 }}>
              {(m.size / 1024 / 1024).toFixed(2)} MB
            </div>
            <div style={{ fontSize: 10, opacity: 0.4 }}>
              {m.tags.join(", ")}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
