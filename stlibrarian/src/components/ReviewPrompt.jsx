import { Button } from "./controls/index.jsx";

/**
 * The ask. It appears once, it takes no over the first time, and it never
 * comes back -- the app has one chance at this and should not spend it
 * nagging.
 */
export default function ReviewPrompt({ onAccept, onDismiss }) {
  return (
    <aside className="ask" role="note">
      <div className="ask__body">
        <strong className="ask__title">Is this working for you?</strong>
        <span className="ask__say">
          You have a real library in here now. If you have two minutes, a review
          helps other people find this.
        </span>
      </div>
      <div className="ask__actions">
        <Button variant="ghost" size="sm" onClick={onDismiss}>
          No thanks
        </Button>
        <Button size="sm" onClick={onAccept}>
          Write one
        </Button>
      </div>
    </aside>
  );
}
