// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/shared/link-resolution.ts (lines 1-9) and apps/desktop/src/shared/runtime-ipc.ts (lines 1959 and 1965-2001) at ebde9907.
// Changed: only the types the lifted files import are kept; the rest of both files is left out.

export type LinkKind = 'conversation' | 'agent' | 'piece'

export interface LinkResolution {
  readonly state: 'available' | 'archived' | 'deleted' | 'unavailable'
  readonly kind: LinkKind
  readonly title: string | null
  readonly status: string | null
  readonly opens: { readonly sessionId: string } | { readonly identityId: string } | null
}

export type RevealResult = { ok: true; path: string } | { ok: false; reason: 'no-path' | 'gone' }

/**
 * The path-like texts the window found in ONE message, as written. A relative one is read from the
 * conversation's working folder; with no conversation, only absolute and `~` paths are read.
 */
export interface MessagePathAsk {
  readonly sessionId: string | null
  readonly texts: readonly string[]
}

/** One path-like text, as main resolved it now: nothing, or a file or a folder that is there. */
export type MessagePath =
  | { readonly text: string; readonly found: false }
  | {
      readonly text: string
      readonly found: true
      /** The full path, for the hover. */
      readonly absolute: string
      /** The last part of the path, for the link's name. */
      readonly name: string
      readonly kind: 'file' | 'folder'
      /** Whether a press opens it: only a file whose type is on main's document allowlist. */
      readonly opens: boolean
    }

/** One path a message wrote, for a press: the conversation it is read from, and the text. */
export interface MessagePathTarget {
  readonly sessionId: string | null
  readonly text: string
}

/**
 * What a press on a path did: opened a document in its default app, or showed the path in the file
 * manager. `gone` is nothing at it now; `no-path` is text that names no path; `failed` is the
 * computer refusing to open it.
 */
export type OpenMessagePathResult =
  { ok: true; did: 'opened' | 'shown' } | { ok: false; reason: 'gone' | 'no-path' | 'failed' }
