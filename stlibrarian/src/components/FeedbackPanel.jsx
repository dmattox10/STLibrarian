import { Button, Panel } from "./controls/index.jsx";

/** Always there, so nobody has to wait to be asked. */
export default function FeedbackPanel({ onOpen }) {
  return (
    <Panel title="Feedback">
      <div className="stack">
        <span className="dim" style={{ fontSize: 11, lineHeight: 1.5 }}>
          Tell us what broke, what is missing, or how it is going. Reviews are
          read by a person before any of them go on the site.
        </span>
        <div className="row" style={{ gap: 6 }}>
          <Button size="sm" onClick={() => onOpen("review")}>
            Write a review
          </Button>
          <Button size="sm" variant="ghost" onClick={() => onOpen("problem")}>
            Report a problem
          </Button>
        </div>
      </div>
    </Panel>
  );
}
