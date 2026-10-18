// A VOICE-KEYER DELETE THAT CAN BE TAKEN BACK. The slot's ✕ sat right under its ▶, and a
// contest tester wiped a recording twice reaching for Play. The backend's clear deletes the WAV
// on the spot, so the Undo cannot be made there: instead the clear is HELD. A Delete marks the
// slot pending — the keyer shows it empty and will not play it — and raises a toast with Undo;
// only when the window passes is the backend told to clear. Undo drops the mark, and the slot
// is the same recording under the same name because nothing ever touched it.
//
// The marks live here, not in the keyer, because the toast outlives the pane: hiding the
// keyer (or leaving Phone) inside the window changes nothing about the delete. Quitting the app
// inside the window leaves the recording on disk, which is the safe side.
import { clearVoiceMessage } from '../api'
import type { VoiceMessage } from '../types'
import { dismissToast, pushToast, withErrorToast } from '../toast'
import { t } from '../i18n'

/** How long a Delete can be undone. */
export const VOICE_UNDO_MS = 8000

interface Pending {
  label: string
  timer: number
  toast: number
  /** Set once the backend clear has started: from then on there is nothing to undo. */
  clearing?: Promise<void>
}

const pending = new Map<number, Pending>()
type Listener = (list: VoiceMessage[] | null) => void
const listeners = new Set<Listener>()

function emit(list: VoiceMessage[] | null): void {
  for (const fn of listeners) fn(list)
}

/** Slots whose recording is deleted but still inside its Undo window. */
export function pendingVoiceDeletes(): ReadonlySet<number> {
  return new Set(pending.keys())
}

/** Called when the pending set changes, with the backend's slot list when a clear landed. */
export function subscribeVoiceDeletes(fn: Listener): () => void {
  listeners.add(fn)
  return () => {
    listeners.delete(fn)
  }
}

/** Delete `slot`'s recording once the Undo window passes. `label` names it in the toast. */
export function deleteVoiceSlot(slot: number, label: string): void {
  if (pending.has(slot)) return
  // The toast is sticky and goes with the mark, so the two cannot drift apart by a tick.
  const toast = pushToast(t('phone.keyer.deleted', { slot, label }), 'info', 0, {
    actionLabel: t('phone.keyer.undo'),
    action: () => undo(slot),
  })
  const timer = window.setTimeout(() => void finishVoiceDelete(slot), VOICE_UNDO_MS)
  pending.set(slot, { label, timer, toast })
  emit(null)
}

function undo(slot: number): void {
  const p = pending.get(slot)
  if (!p || p.clearing) return
  window.clearTimeout(p.timer)
  pending.delete(slot)
  emit(null)
  pushToast(t('phone.keyer.restored', { slot, label: p.label }), 'success', 2500)
}

/** A new take or import is about to be saved into `slot`: its save overwrites the file, so a
 *  pending delete is dropped WITHOUT a clear, and the window's end cannot wipe the new take. If
 *  the save then fails, the old recording is still there, which is the safe side. Waits for a
 *  clear already under way, so it cannot land after the save. */
export function supersedeVoiceDelete(slot: number): Promise<void> {
  const p = pending.get(slot)
  if (!p) return Promise.resolve()
  if (p.clearing) return p.clearing
  window.clearTimeout(p.timer)
  dismissToast(p.toast)
  pending.delete(slot)
  emit(null)
  return Promise.resolve()
}

/** Make a pending delete final now, when its window passes. Resolves once the backend has
 *  answered; does nothing when `slot` has no pending delete. */
function finishVoiceDelete(slot: number): Promise<void> {
  const p = pending.get(slot)
  if (!p) return Promise.resolve()
  if (p.clearing) return p.clearing
  window.clearTimeout(p.timer)
  dismissToast(p.toast)
  p.clearing = withErrorToast(() => clearVoiceMessage(slot), t('phone.keyer.clearFailed')).then(
    (list) => {
      pending.delete(slot)
      emit(list)
    },
  )
  return p.clearing
}
