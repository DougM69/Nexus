// BAND MAP FREQUENCY MARKERS, N1MM-style: "Mark (Alt+M) – Marks the current frequency in the Bandmap
// as being in use" (N1MM Logger+ manual, Entry Window ▸ Action Button Row), here with the call in the
// contest entry beside it when there is one.
//
//   Alt+M in a contest entry, or a right-click on the band map, marks the station's dial now;
//   a click or Enter on a marker jumps back to it (an operator QSY, refused while transmitting);
//   Delete on a marker removes it, and logging its call on its band removes it too.
//
// The markers are the station's (`freq_markers.rs`, one file per contest session), never posted and
// never in the snapshot: the band map window asks for them. The entry is in the main window (or the
// logger window) and the band map in a window of its own, so the entry PUBLISHES its call to a shared
// key the band map reads at its right-click, and a change says so on another shared key, which the
// band map hears as a `storage` event and reads the list again.
import { addFreqMarker, type FreqMarker } from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

/** The contest entry's call, as the entry on screen last typed it. SHARED (storage-scope.test.ts). */
export const ENTRY_CALL_KEY = 'nexus.contest.entryCall'
/** Written at every marker change, for another window's band map to read the list again. SHARED. */
export const MARKERS_CHANGED_KEY = 'nexus.freqMarkers.changed'

type KeyChord = Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>

/** Alt+M, and nothing else held (Ctrl with Alt is AltGr). The letter decides, and the key's place only
 *  where the layout types no Latin letter there (a Mac's Option+M types µ). */
export function isMarkKey(e: KeyChord): boolean {
  if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return false
  return /^[a-z]$/i.test(e.key) ? e.key.toLowerCase() === 'm' : e.code === 'KeyM'
}

export function publishEntryCall(call: string): void {
  try {
    // Only a change is written: every write is an event for each other window to wake for.
    const v = call.trim().toUpperCase()
    if ((localStorage.getItem(ENTRY_CALL_KEY) ?? '') !== v) localStorage.setItem(ENTRY_CALL_KEY, v)
  } catch {
    /* storage off: the band map marks with no call */
  }
}

export function entryCall(): string {
  try {
    return localStorage.getItem(ENTRY_CALL_KEY) ?? ''
  } catch {
    return ''
  }
}

export function announceMarkersChanged(): void {
  try {
    localStorage.setItem(MARKERS_CHANGED_KEY, String(Date.now()))
  } catch {
    /* the band map's own poll still finds it */
  }
}

/** A marker refusal token as a sentence. */
export function markerRefusal(err: unknown): string {
  const token = String(err)
  if (token.includes('markerNoContest')) return t('freqMarker.refused.noContest')
  if (token.includes('markerTransmitting')) return t('freqMarker.refused.transmitting')
  if (token.includes('markerOffBand')) return t('freqMarker.refused.offBand')
  if (token.includes('markerNotFound')) return t('freqMarker.refused.gone')
  return t('freqMarker.refused.failed', { reason: token })
}

/** The marker's frequency as the band map writes it (MHz, three decimals and the 100 Hz digit). */
export function markerMhz(m: Pick<FreqMarker, 'dialHz'>): string {
  return (m.dialHz / 1e6).toFixed(4)
}

/** Mark the station's dial now with `call`, and say what was marked or why not. */
export async function markHere(call: string): Promise<FreqMarker[] | null> {
  const c = call.trim().toUpperCase()
  try {
    const list = await addFreqMarker(c)
    announceMarkersChanged()
    const made = list[list.length - 1]
    const freq = made ? markerMhz(made) : ''
    pushToast(c ? t('freqMarker.marked', { call: c, freq }) : t('freqMarker.markedBare', { freq }), 'success')
    return list
  } catch (e) {
    pushToast(markerRefusal(e), 'error')
    return null
  }
}
