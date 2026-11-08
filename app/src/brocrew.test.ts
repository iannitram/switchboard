/// <reference types="vite/client" />
// SHELL-2 (#12): BroCrew's plain-TypeScript chat parts, lifted into app/src/brocrew/. Locked test (R28).
import { expect, test } from "vitest";
import type { SendBlock } from "./brocrew/composer-notice";
import {
  LINK_FAILED,
  LINK_UNAVAILABLE,
  MESSAGE_LINK_ATTRIBUTE,
  internalLinkStart,
  linkFailureReason,
} from "./brocrew/message-links";
import { answerFor, fileManagerWord, pressFailedWords, showName } from "./brocrew/message-paths";
import type { LinkResolution, MessagePath } from "./brocrew/ipc-types";
import composerNoticeSrc from "./brocrew/composer-notice.ts?raw";
import ipcTypesSrc from "./brocrew/ipc-types.ts?raw";
import messageLinksSrc from "./brocrew/message-links.ts?raw";
import messagePathsSrc from "./brocrew/message-paths.ts?raw";

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object") {
    Object.values(value).forEach(deepFreeze);
    Object.freeze(value);
  }
  return value;
}

const LIFTED = deepFreeze({
  "composer-notice.ts": composerNoticeSrc,
  "ipc-types.ts": ipcTypesSrc,
  "message-links.ts": messageLinksSrc,
  "message-paths.ts": messagePathsSrc,
});

// The notice (the same three markers tools/check-notices.py looks for).
for (const [name, src] of Object.entries(LIFTED)) {
  test(`${name} starts with its Apache-2.0 notice, the BroCrew credit and what changed`, () => {
    const head = src.split("\n").slice(0, 15).join("\n");
    expect(head).toContain("SPDX-License-Identifier: Apache-2.0");
    expect(head).toContain("https://github.com/brocrew/brocrew");
    expect(head).toContain("Changed:");
  });

  test(`${name} imports nothing from outside app/src/brocrew`, () => {
    const froms = [...src.matchAll(/from\s+['"]([^'"]+)['"]/g)].map((m) => m[1]);
    for (const from of froms) expect(from.startsWith("./")).toBe(true);
  });
}

// message-paths.ts
test("fileManagerWord names the file manager from navigator.platform", () => {
  expect(fileManagerWord("MacIntel")).toBe("Finder");
  expect(fileManagerWord("Win32")).toBe("Explorer");
  expect(fileManagerWord("Linux x86_64")).toBe("your file manager");
});

test("showName says where a path will be shown", () => {
  expect(showName("notes.md", "Finder")).toBe("Show notes.md in Finder");
});

test("answerFor keeps an answer only when its text matches", () => {
  const found: MessagePath = deepFreeze({
    text: "~/a.md",
    found: true,
    absolute: "/home/u/a.md",
    name: "a.md",
    kind: "file",
    opens: true,
  });
  expect(answerFor("~/a.md", found)).toBe(found);
  expect(answerFor("~/b.md", found)).toEqual({ text: "~/b.md", found: false });
  expect(answerFor("~/c.md", undefined)).toEqual({ text: "~/c.md", found: false });
});

test("pressFailedWords: a path that is gone", () => {
  expect(pressFailedWords("gone", "file", "Finder", "open")).toBe(
    "That file isn't there any more. Ask the agent where it is now.",
  );
});

test("pressFailedWords: no path, in Switchboard's name", () => {
  expect(pressFailedWords("no-path", "folder", "Finder", "show")).toBe(
    "Switchboard can't tell where that folder is from here. Ask the agent for its full path.",
  );
});

test("pressFailedWords: an open that failed", () => {
  expect(pressFailedWords("failed", "file", "your file manager", "open")).toBe(
    "That file didn't open. Copy its path and open it from your file manager.",
  );
});

test("pressFailedWords: a show that failed starts with the capitalised file manager", () => {
  expect(pressFailedWords("failed", "folder", "your file manager", "show")).toBe(
    "Your file manager didn't show that folder. Try again.",
  );
});

// message-links.ts
test("the link constants are BroCrew's", () => {
  expect(MESSAGE_LINK_ATTRIBUTE).toBe("data-brocrew-target");
  expect(LINK_UNAVAILABLE).toBe("Unavailable");
  expect(LINK_FAILED).toBe("Couldn't open this link.");
});

test("linkFailureReason names a deleted target by its kind", () => {
  const deleted = (kind: LinkResolution["kind"]): LinkResolution =>
    deepFreeze({ state: "deleted", kind, title: null, status: null, opens: null });
  expect(linkFailureReason(deleted("conversation"))).toBe("This conversation was deleted.");
  expect(linkFailureReason(deleted("agent"))).toBe("This agent was deleted.");
  expect(linkFailureReason(deleted("piece"))).toBe("This work no longer exists.");
});

test("linkFailureReason gives the plain sentence for any other state", () => {
  for (const state of ["available", "archived", "unavailable"] as const) {
    const answer: LinkResolution = deepFreeze({ state, kind: "agent", title: null, status: null, opens: null });
    expect(linkFailureReason(answer)).toBe("Couldn't open this link.");
  }
});

test("internalLinkStart opens a link button for a conversation", () => {
  const html = internalLinkStart('brocrew://conversation/a"b', (t) => t.replace(/"/g, "&quot;"));
  expect(html).toBe(
    '<button type="button" role="link" class="message-link" data-cy="message-link" title="" data-brocrew-target="brocrew://conversation/a&quot;b">' +
      '<svg class="icon message-link-indicator" role="img" aria-label="Conversation link"><use href="#i-chat"></use></svg>',
  );
});

test("internalLinkStart marks an agent link, whatever its case", () => {
  const html = internalLinkStart("BroCrew://Agent/x", (t) => t);
  expect(html).toContain('aria-label="Agent link"');
  expect(html).toContain('<use href="#i-user">');
});

// composer-notice.ts
test("a SendBlock carries the words and an optional fix", () => {
  let pressed = 0;
  const block: SendBlock = { words: "Pick a model first.", fix: { label: "Pick", press: () => pressed++ } };
  block.fix?.press();
  expect(pressed).toBe(1);
  const plain: SendBlock = { words: "Offline." };
  expect(plain.fix).toBeUndefined();
});
