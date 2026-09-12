import { useEffect, useRef, useState } from "react";
import { Button, CloseIcon, Toggle } from "./controls/index.jsx";
import { publishable, send } from "../lib/feedback.js";

const KINDS = [
  { value: "review", label: "Review", blurb: "How it is working out for you." },
  { value: "problem", label: "Problem", blurb: "Something went wrong." },
  { value: "idea", label: "Idea", blurb: "Something you wish it did." },
];

const PROMPTS = {
  review: "What is it like to use?",
  problem: "What happened, and what did you expect instead?",
  idea: "What would you like it to do?",
};

/** Five pressable slabs. A review without a rating is not a review. */
function Stars({ value, onChange }) {
  return (
    <div className="stars" role="radiogroup" aria-label="Rating">
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          type="button"
          role="radio"
          aria-checked={value === n}
          aria-label={`${n} out of 5`}
          className="stars__pip"
          data-lit={n <= value ? "yes" : "no"}
          onClick={() => onChange(n)}
        >
          {n <= value ? "★" : "☆"}
        </button>
      ))}
    </div>
  );
}

/**
 * Saying something back to the people who wrote this.
 *
 * Nothing typed here is public on its own. A review reaches the website only
 * if a person approves it *and* the box below was ticked; a problem report
 * never becomes a testimonial regardless of how kindly it is worded. Contact
 * details are for replying and are never published.
 */
export default function FeedbackDialog({ api, initialKind = "review", onClose }) {
  const [kind, setKind] = useState(initialKind);
  const [rating, setRating] = useState(0);
  const [body, setBody] = useState("");
  const [author, setAuthor] = useState("");
  const [printer, setPrinter] = useState("");
  const [contact, setContact] = useState("");
  const [mayPublish, setMayPublish] = useState(true);
  const [state, setState] = useState("editing");
  const [error, setError] = useState("");
  const [info, setInfo] = useState(["", ""]);
  const firstField = useRef(null);

  useEffect(() => {
    api.appInfo?.().then(setInfo).catch(() => {});
  }, [api]);

  useEffect(() => {
    firstField.current?.focus();
    const onKey = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const isReview = kind === "review";
  const ready = body.trim().length >= 4 && (!isReview || rating > 0);

  async function submit(e) {
    e.preventDefault();
    if (!ready || state === "sending") return;
    setState("sending");
    setError("");
    try {
      await send({
        kind,
        rating: isReview ? rating : null,
        body,
        author,
        printer,
        contact,
        // Only a review can be published, whatever this box says.
        may_publish: publishable(kind, mayPublish),
        app_version: info[0] || "",
        platform: info[1] || "",
      });
      setState("sent");
    } catch (err) {
      setError(err.message || "Could not send that.");
      setState("editing");
    }
  }

  return (
    <div className="modal" role="dialog" aria-modal="true" aria-label="Send feedback">
      <div className="modal__scrim" onClick={onClose} />
      <form className="modal__card" onSubmit={submit}>
        <header className="modal__head">
          <h2 className="modal__title">
            {state === "sent" ? "Thank you" : "Tell us how it is going"}
          </h2>
          <button type="button" className="modal__x" onClick={onClose} aria-label="Close">
            <CloseIcon />
          </button>
        </header>

        {state === "sent" ? (
          <div className="modal__body stack">
            <p className="modal__say">
              That is on its way.{" "}
              {isReview && mayPublish
                ? "If it goes up on the site, it will be after a person has read it."
                : "It goes to the people who make this and nowhere else."}
            </p>
            <div className="modal__foot">
              <Button type="button" onClick={onClose}>
                Close
              </Button>
            </div>
          </div>
        ) : (
          <div className="modal__body stack">
            <div className="kinds">
              {KINDS.map((k) => (
                <button
                  key={k.value}
                  type="button"
                  className="kind"
                  aria-pressed={kind === k.value}
                  onClick={() => setKind(k.value)}
                >
                  <strong>{k.label}</strong>
                  <span>{k.blurb}</span>
                </button>
              ))}
            </div>

            {isReview ? (
              <label className="fbrow">
                <span className="fbrow__label">Rating</span>
                <Stars value={rating} onChange={setRating} />
              </label>
            ) : null}

            <label className="stack" style={{ gap: 6 }}>
              <span className="fbrow__label">{PROMPTS[kind]}</span>
              <textarea
                ref={firstField}
                className="gc-textarea"
                rows={6}
                maxLength={4000}
                value={body}
                onChange={(e) => setBody(e.target.value)}
                placeholder={
                  isReview
                    ? "What you print, and how this fits into it."
                    : "Anything that helps us find it."
                }
              />
            </label>

            <div className="fbgrid">
              <label className="stack" style={{ gap: 6 }}>
                <span className="fbrow__label">Name {isReview ? "" : "(optional)"}</span>
                <input
                  className="gc-textinput"
                  maxLength={80}
                  value={author}
                  onChange={(e) => setAuthor(e.target.value)}
                  placeholder="Shown with a published review"
                />
              </label>
              <label className="stack" style={{ gap: 6 }}>
                <span className="fbrow__label">Printer (optional)</span>
                <input
                  className="gc-textinput"
                  maxLength={80}
                  value={printer}
                  onChange={(e) => setPrinter(e.target.value)}
                  placeholder="Bambu P1S, Prusa MK4…"
                />
              </label>
            </div>

            <label className="stack" style={{ gap: 6 }}>
              <span className="fbrow__label">Email, if you want an answer (optional)</span>
              <input
                className="gc-textinput"
                type="email"
                maxLength={200}
                value={contact}
                onChange={(e) => setContact(e.target.value)}
                placeholder="Never published"
              />
            </label>

            {isReview ? (
              <Toggle checked={mayPublish} onChange={setMayPublish}>
                You may show this on stlibrarian.com
              </Toggle>
            ) : null}

            <p className="modal__fine">
              Sent with the app version and platform{info[0] ? ` (${info[0]}, ${info[1]})` : ""}.
              Nothing about your library, your files or your folders leaves this computer.
            </p>

            {error ? <div className="gc-alert">{error}</div> : null}

            <div className="modal__foot">
              <Button type="button" variant="ghost" onClick={onClose}>
                Not now
              </Button>
              <Button type="submit" disabled={!ready || state === "sending"}>
                {state === "sending" ? "Sending…" : "Send"}
              </Button>
            </div>
          </div>
        )}
      </form>
    </div>
  );
}
