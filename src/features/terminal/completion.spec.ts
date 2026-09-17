import { expect, it } from "vitest";
import { replaceCompletion } from "./completion";

it("keeps value-taking flags and quoted directories open for further typing", () => {
  const completion = { start: 8, end: 8, items: [], hasMore: false };
  expect(replaceCompletion("git log ", completion, "--max-count=", true).command).toBe("git log --max-count=");
  expect(replaceCompletion("git add ", completion, '"space dir/"', true).command).toBe('git add "space dir/"');
});
