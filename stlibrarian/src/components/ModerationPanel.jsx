import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Panel, TrashIcon } from "./controls/index.jsx";
import { formatDate } from "../lib/format.js";

const QUEUES = [
  { value: "pending", label: "New" },
  { value: "approved", label: "Approved" },
  { value: "rejected", label: "Rejected" },
];

const KIND_LABEL = { review: "Review", problem: "Problem", idea: "Idea" };

/**
 * The moderation queue. Pro only, administrators only.
 *
 * Every submission arrives here and stays private. Approving a review is what
 * puts it on the website, and only if its author released it -- the "not for
 * publication" mark below is the author's choice, not ours to overrule, so
 * approving one of those simply files it as read.
 */
export default function ModerationPanel({ api }) {
  const [queue, setQueue] = useState("pending");
  const [items, setItems] = useState([]);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    (status) =>
      api
        .feedbackQueue(status)
        .then(setItems)
        .catch((e) => {
          setItems([]);
          setError(String(e.message || e));
        }),
    [api]
  );

  useEffect(() => {
    let cancelled = false;
    api
      .feedbackQueue(queue)
      .then((rows) => !cancelled && setItems(rows))
      .catch(() => !cancelled && setItems([]));
    return () => {
      cancelled = true;
    };
  }, [api, queue]);

  const guard = async (action) => {
    setError(null);
    setBusy(true);
    try {
      await action();
      await load(queue);
    } catch (e) {
      setError(String(e.message || e));
    } finally {
      setBusy(false);
    }
  };

  const pending = queue === "pending";

  return (
    <Panel
      title="Feedback"
      actions={
        <span className="gc-chip num">{items.length}</span>
      }
    >
      <div className="stack">
        <div className="modqueue">
          {QUEUES.map((q) => (
            <button
              key={q.value}
              type="button"
              className="modqueue__tab"
              aria-pressed={queue === q.value}
              onClick={() => setQueue(q.value)}
            >
              {q.label}
            </button>
          ))}
        </div>

        {error ? <Alert>{error}</Alert> : null}

        {items.length === 0 ? (
          <span className="dim" style={{ fontSize: 11 }}>
            {pending ? "Nothing waiting." : "Nothing here."}
          </span>
        ) : null}

        {items.map((f) => (
          <article key={f.id} className="modrow">
            <header className="modrow__top">
              <span className="modrow__kind" data-kind={f.kind}>
                {KIND_LABEL[f.kind] || f.kind}
              </span>
              {f.rating ? (
                <span className="modrow__rating num" title={`${f.rating} of 5`}>
                  {"★".repeat(f.rating)}
                  <span className="dim">{"★".repeat(5 - f.rating)}</span>
                </span>
              ) : null}
              <span className="modrow__when num">{formatDate(f.submitted_at)}</span>
              <button
                type="button"
                className="collrow__x"
                title="Delete for good"
                disabled={busy}
                onClick={() => guard(() => api.deleteFeedback(f.id))}
              >
                <TrashIcon />
              </button>
            </header>

            <p className="modrow__body">{f.body}</p>

            <div className="modrow__meta">
              <span>{f.author || "Anonymous"}</span>
              {f.printer ? <span>{f.printer}</span> : null}
              {f.app_version ? (
                <span className="num">
                  {f.app_version} · {f.platform}
                </span>
              ) : null}
              {f.contact ? (
                <a className="modrow__contact" href={`mailto:${f.contact}`}>
                  {f.contact}
                </a>
              ) : null}
            </div>

            <div className="modrow__flags">
              {f.kind === "review" && f.may_publish ? (
                <span className="modrow__flag" data-tone="go">
                  Author released this for the site
                </span>
              ) : (
                <span className="modrow__flag">Private, not for publication</span>
              )}
            </div>

            <div className="row" style={{ gap: 6 }}>
              {queue !== "approved" ? (
                <Button
                  size="sm"
                  disabled={busy}
                  onClick={() => guard(() => api.moderateFeedback(f.id, "approved"))}
                >
                  {f.kind === "review" && f.may_publish ? "Approve & publish" : "Approve"}
                </Button>
              ) : null}
              {queue !== "rejected" ? (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => guard(() => api.moderateFeedback(f.id, "rejected"))}
                >
                  Reject
                </Button>
              ) : null}
              {queue !== "pending" ? (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => guard(() => api.moderateFeedback(f.id, "pending"))}
                >
                  Back to queue
                </Button>
              ) : null}
            </div>
          </article>
        ))}
      </div>
    </Panel>
  );
}
