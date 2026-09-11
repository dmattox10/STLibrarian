/**
 * The two rules the client must never get wrong: when it is allowed to ask,
 * and what it is allowed to offer for publication.
 *
 * Run with `npm test`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  EARNED_MODELS,
  EARNED_PRINTS,
  earned,
  publishable,
} from "./feedback.js";

test("a fresh install is never asked", () => {
  assert.equal(earned({ models: 0, printed: 0 }), false);
  assert.equal(earned(undefined), false);
  assert.equal(earned({}), false);
});

test("a big import with nothing printed is a folder, not a habit", () => {
  assert.equal(earned({ models: 5000, printed: 0 }), false);
  assert.equal(earned({ models: 5000, printed: EARNED_PRINTS - 1 }), false);
});

test("a few prints against a tiny library is not enough either", () => {
  assert.equal(earned({ models: EARNED_MODELS - 1, printed: 99 }), false);
});

test("both conditions, exactly at the line, earns the question", () => {
  assert.equal(earned({ models: EARNED_MODELS, printed: EARNED_PRINTS }), true);
});

test("only a review can be offered for publication", () => {
  assert.equal(publishable("review", true), true);
  assert.equal(publishable("review", false), false);
});

test("a glowing bug report is still a bug report", () => {
  assert.equal(publishable("problem", true), false);
  assert.equal(publishable("idea", true), false);
});

test("an unticked box is never read as consent", () => {
  // Anything that is not a literal true is not permission.
  for (const value of [undefined, null, 0, "", "yes", 1]) {
    assert.equal(publishable("review", value), false, `${JSON.stringify(value)}`);
  }
});
