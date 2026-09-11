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
