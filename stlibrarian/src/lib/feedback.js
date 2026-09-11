/**
 * Where reviews and problem reports go.
 *
 * A copy of the app on somebody's bench has no server of its own, so this
 * points at one central instance. Overridable, which is how it gets tested
 * without posting into the real moderation queue.
 */
const DEFAULT_ENDPOINT = "https://stlibrarian.com/api/feedback";

export function endpoint() {
  try {
    return localStorage.getItem("stlibrarian.feedbackUrl") || DEFAULT_ENDPOINT;
  } catch {
    return DEFAULT_ENDPOINT;
  }
}

export async function send(entry) {
  const res = await fetch(endpoint(), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(entry),
  });
  if (res.status === 429) {
    throw new Error("That is a lot of feedback at once. Try again a bit later.");
  }
  if (!res.ok) {
    let message = `Could not send that (${res.status})`;
    try {
      const payload = await res.json();
      if (payload?.error) message = payload.error;
    } catch {
      /* not a JSON error body */
    }
    throw new Error(message);
  }
  return res.json().catch(() => ({}));
}

/** A library this size, with this many prints behind it, is someone using it. */
export const EARNED_MODELS = 25;
export const EARNED_PRINTS = 3;

/**
 * Whether the app has any business asking for a review yet.
 *
 * Asking a fresh install is asking someone to rate a room they have not walked
 * into. The question is only worth putting once there is an answer to give: a
 * real library, and prints actually logged against it. Both conditions, not
 * either -- a big import with nothing printed is a folder, not a habit.
 */
export function earned(stats) {
  return (stats?.models ?? 0) >= EARNED_MODELS && (stats?.printed ?? 0) >= EARNED_PRINTS;
}

/**
 * What the app is allowed to mark for publication.
 *
 * Only a review can ever become a testimonial, and only if its author released
 * it. A glowing bug report is still a bug report.
 */
export function publishable(kind, mayPublish) {
  return kind === "review" && mayPublish === true;
}

/** Whether the app has already asked this person for a review. */
export function asked() {
  try {
    return localStorage.getItem("stlibrarian.reviewAsked") === "yes";
  } catch {
    return false;
  }
}

export function markAsked() {
  try {
    localStorage.setItem("stlibrarian.reviewAsked", "yes");
  } catch {
    /* storage disabled; it will ask again, which is a small cost */
  }
}
