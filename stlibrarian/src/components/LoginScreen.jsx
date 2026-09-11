import { useState } from "react";
import { Alert, Button, CubeIcon, Field } from "./controls/index.jsx";
import { isDesktop, readRemote, writeRemote } from "../data/adapter.js";

export default function LoginScreen({ api, onSignedIn }) {
  // A desktop client pointed at a server it cannot reach would otherwise be
  // stuck here forever: the control that switches libraries lives in the rail,
  // and the rail is not rendered until you are signed in.
  const remote = isDesktop() ? readRemote() : null;
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(null);

  const submit = async (event) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const { user } = await api.login(username, password);
      onSignedIn(user);
    } catch (e) {
      setError(e?.message || "Could not sign in.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login">
      <form className="gc-panel login__card" onSubmit={submit}>
        <div className="gc-panel__body stack" style={{ gap: 14 }}>
          <div className="row" style={{ gap: 10 }}>
            <span className="brand__mark">
              <CubeIcon />
            </span>
            <span className="brand__name">
              ST<em>Librarian</em>
            </span>
          </div>

          <p className="dim" style={{ margin: 0, fontSize: 12, lineHeight: 1.6 }}>
            {remote ? (
              <>
                Sign in to <strong style={{ color: "var(--nord8)" }}>{remote}</strong>.
              </>
            ) : (
              "Sign in to this library server."
            )}
          </p>

          <Field>
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="Username"
              autoComplete="username"
              aria-label="Username"
              autoFocus
            />
          </Field>
          <Field>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Password"
              autoComplete="current-password"
              aria-label="Password"
            />
          </Field>

          <Alert>{error}</Alert>

          <Button
            variant="primary"
            type="submit"
            disabled={busy || !username || !password}
            style={{ width: "100%" }}
          >
            {busy ? "Signing in" : "Sign in"}
          </Button>

          <p className="dim" style={{ margin: 0, fontSize: 11, lineHeight: 1.6 }}>
            First run? The server printed a generated admin password to its
            console on startup.
          </p>

          {remote ? (
            <Button
              variant="ghost"
              size="sm"
              type="button"
              onClick={() => {
                writeRemote(null);
                window.location.reload();
              }}
            >
              Use this computer instead
            </Button>
          ) : null}
        </div>
      </form>
    </div>
  );
}
