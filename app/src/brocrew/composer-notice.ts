// SPDX-License-Identifier: Apache-2.0
// Copyright the BroCrew authors (https://github.com/brocrew/brocrew).
// Lifted from BroCrew apps/desktop/src/renderer/src/lib/composer-notice.ts at ebde9907.
// Changed: nothing but this notice.

/** A reason Send can't go, in a person's words, with the press that fixes it when there is one. */
export interface SendBlock {
  words: string
  fix?: { label: string; press: () => void }
}
