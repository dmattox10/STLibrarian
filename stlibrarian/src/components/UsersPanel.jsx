import { useEffect, useState } from "react";
import { Alert, Button, Field, Panel, TrashIcon } from "./controls/index.jsx";

/**
 * Who can get in, and what they may do. Pro only -- the desktop app has one
 * person and no accounts.
 *
 * Two roles, deliberately. An administrator can scan, organise, delete models
 * and manage people; everyone else can browse, tag, collect and log prints.
 * More roles than that is a permissions matrix nobody maintains.
 */
export default function UsersPanel({ api, me, onChanged }) {
  const [users, setUsers] = useState([]);
  const [adding, setAdding] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [resetting, setResetting] = useState(null);
  const [newPassword, setNewPassword] = useState("");
  const [confirming, setConfirming] = useState(null);
  const [error, setError] = useState(null);

  const load = () =>
    api
      .users()
      .then(setUsers)
      .catch(() => setUsers([]));

  useEffect(() => {
    let cancelled = false;
    api
      .users()
      .then((u) => !cancelled && setUsers(u))
      .catch(() => !cancelled && setUsers([]));
    return () => {
      cancelled = true;
    };
  }, [api]);

  const guard = async (action) => {
    setError(null);
    try {
      await action();
      await load();
      onChanged?.();
    } catch (e) {
      setError(e?.message || "That did not work.");
    }
  };

  const admins = users.filter((u) => u.role === "admin").length;

  return (
    <Panel
      title="People"
      actions={
        <Button variant="ghost" size="sm" onClick={() => setAdding((v) => !v)}>
          {adding ? "Cancel" : "Add"}
        </Button>
      }
    >
      <div className="stack">
        {adding ? (
          <div className="stack" style={{ gap: 6 }}>
            <Field>
              <input
                value={username}
                placeholder="Username"
                onChange={(e) => setUsername(e.target.value)}
                aria-label="Username"
                autoFocus
              />
            </Field>
            <Field>
              <input
                type="password"
                value={password}
                placeholder="Password, at least 8 characters"
                onChange={(e) => setPassword(e.target.value)}
                aria-label="Password"
              />
            </Field>
            <Button
              variant="primary"
              size="sm"
              disabled={!username.trim() || password.length < 8}
              onClick={() =>
                guard(async () => {
                  await api.createUser(username.trim(), password, "user");
                  setUsername("");
                  setPassword("");
                  setAdding(false);
                })
              }
            >
              Add person
            </Button>
          </div>
        ) : null}

        {users.map((u) => (
          <div className="userrow" key={u.id}>
            <div className="userrow__body">
              <span className="userrow__name">
                {u.username}
                {me?.id === u.id ? <span className="dim"> (you)</span> : null}
              </span>
              <button
                type="button"
                className="userrow__role"
                aria-pressed={u.role === "admin"}
                // The last administrator cannot step down: a server with
                // nobody able to promote anyone is only fixable by hand.
                disabled={u.role === "admin" && admins <= 1}
                title={
                  u.role === "admin" && admins <= 1
                    ? "The only administrator"
                    : "Change role"
                }
                onClick={() =>
                  guard(() =>
                    api.setUserRole(u.id, u.role === "admin" ? "user" : "admin")
                  )
                }
              >
                {u.role}
              </button>
            </div>
            <button
              type="button"
              className="collrow__x"
              aria-label={`Reset password for ${u.username}`}
              title="Reset password"
              onClick={() => {
                setResetting(resetting === u.id ? null : u.id);
                setNewPassword("");
              }}
            >
              &#9998;
            </button>
            <button
              type="button"
              className="collrow__x"
              aria-label={`Remove ${u.username}`}
              title="Remove. Their print history is kept."
              onClick={() => setConfirming(u.id)}
            >
              <TrashIcon />
            </button>
          </div>
        ))}

        {resetting != null ? (
          <Field>
            <input
              type="password"
              value={newPassword}
              placeholder="New password"
              onChange={(e) => setNewPassword(e.target.value)}
              aria-label="New password"
              autoFocus
            />
            <Button
              size="sm"
              disabled={newPassword.length < 8}
              onClick={() =>
                guard(async () => {
                  await api.setUserPassword(resetting, newPassword);
                  setResetting(null);
                  setNewPassword("");
                })
              }
            >
              Set
            </Button>
          </Field>
        ) : null}

        {confirming != null ? (
          <div className="stack" style={{ gap: 6 }}>
            <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
              Remove this person? Their print records stay. A shop&rsquo;s
              account of what came off which printer outlives anyone&rsquo;s
              account.
            </span>
            <div className="row">
              <Button
                variant="danger"
                size="sm"
                onClick={() =>
                  guard(async () => {
                    await api.deleteUser(confirming);
                    setConfirming(null);
                  })
                }
              >
                Remove
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setConfirming(null)}>
                Cancel
              </Button>
            </div>
          </div>
        ) : null}

        <Alert>{error}</Alert>
      </div>
    </Panel>
  );
}
