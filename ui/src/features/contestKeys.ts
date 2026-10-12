// THE CONTEST LOG STRIP'S ALT+W AND CTRL+O, as N1MM Logger+'s key list has them (Ctrl+D, the third
// of its keys here, is `contestRemoval.ts`'s):
//
//   "Ctrl+W or Alt+W = Wipe entry fields. Release a reserved serial number. Ctrl+W is
//   irreversible, Alt+W can be reversed by typing it again" — Alt+W wipes the log line, and a
//   second Alt+W right after puts it back (`LogEntry` keeps the line it wiped);
//   "Ctrl+O = Set operator call" — Ctrl+O takes the caret to the operator box.
//
// THE OPERATOR BOX IS NOT THE STRIP'S. It is the Contest screen's (`FieldDayScoreboard`), so the
// strip only ASKS, by a window event, and the window that holds the strip answers: the main window
// shows its Contest screen, and the contest logger window has one under its entry. Nothing here
// keys, stops or holds back a transmission, and asking changes no mode: the station's FT role is
// what it was.
import { useEffect, useRef } from 'react'

/** The window event a strip's Ctrl+O fires (`askForOperatorBox`). */
export const OPERATOR_BOX_ASK = 'nexus:operator-box'

type KeyChord = Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>

/** `letter` pressed: the letter decides, and the key's place only where the layout types no Latin
 *  letter there — a Mac's Option+W types ∑, a Cyrillic layout's W key types ц. */
const isLetter = (e: KeyChord, letter: string): boolean =>
  /^[a-z]$/i.test(e.key) ? e.key.toLowerCase() === letter : e.code === `Key${letter.toUpperCase()}`

/** Alt+W, and nothing else held: Ctrl with Alt is AltGr, which types characters on many keyboards. */
export function isWipeKey(e: KeyChord): boolean {
  return e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey && isLetter(e, 'w')
}

/** Ctrl+O, and nothing else held. */
export function isOperatorBoxKey(e: KeyChord): boolean {
  return e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey && isLetter(e, 'o')
}

/** Ctrl+O, from a strip: whatever holds this window's operator box answers. */
export function askForOperatorBox(): void {
  window.dispatchEvent(new Event(OPERATOR_BOX_ASK))
}

/** A window's answer to Ctrl+O: `onAsk` runs at each ask while the caller is mounted. */
export function useOperatorBoxAsk(onAsk: () => void): void {
  const ask = useRef(onAsk)
  ask.current = onAsk
  useEffect(() => {
    const answer = () => ask.current()
    window.addEventListener(OPERATOR_BOX_ASK, answer)
    return () => window.removeEventListener(OPERATOR_BOX_ASK, answer)
  }, [])
}
