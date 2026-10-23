// The JS8 compose box's settings and JS8Call's rules for what its one-click buttons do to it —
// pure, so the cockpit's dock and its tests read the same rules.
//
//   • js8ComposeConfig: Settings ▸ JS8's compose fields, at JS8Call 3.0.3's defaults where a
//     settings file (or a Remote page's sample) does not carry them.
//   • js8InsertText: JS8Call's `addMessageText` (mainwindow.cpp:3288-3330) — a button's text goes in
//     at the caret, over the selection, after a space unless the caret starts the box or follows a
//     space; the Directed menu's text replaces the box instead (`clear`).
//   • js8MacroSelected: the selected call's values for `<CALL>`, `<SNR>` and `<TDELTA>` — the To
//     box's call when the station has heard it.
//   • js8CqTextRefused: JS8Call's Settings check on its CQ message.
import type { Js8ActivityRow, Js8Heard, Settings } from './types'
import { JS8_DEFAULT_CQ, JS8_DEFAULT_REPLY, JS8_DEFAULT_SAVED, type Js8MacroInput } from './js8Macros'

export interface Js8ComposeConfig {
  /** JS8Call's CQ message: the CQ button and the CQ repeat. */
  cqText: string
  /** JS8Call's reply message: REPLY. */
  replyText: string
  /** JS8Call's saved messages: the Saved menu, in order. */
  savedMessages: readonly string[]
  /** JS8Call's "Immediately transmit…": the one-click buttons send at once (on) or fill (off). */
  sendAtOnce: boolean
  /** JS8Call 3.0's pills in the compose box. */
  pills: boolean
  /** JS8Call's word suggestions in the compose box. */
  wordSuggestions: boolean
  /** INFO and STATUS as set (an empty STATUS is JS8Call's default). */
  info: string
  status: string
}

/** JS8Call 3.0.3's own defaults. */
export const JS8_COMPOSE_DEFAULTS: Js8ComposeConfig = {
  cqText: JS8_DEFAULT_CQ,
  replyText: JS8_DEFAULT_REPLY,
  savedMessages: JS8_DEFAULT_SAVED,
  sendAtOnce: true,
  pills: true,
  wordSuggestions: true,
  info: '',
  status: '',
}

/** The compose fields of `s`, each at JS8Call's default where `s` does not carry it. */
export function js8ComposeConfig(s: Partial<Settings> | null | undefined): Js8ComposeConfig {
  if (!s) return JS8_COMPOSE_DEFAULTS
  return {
    cqText: s.js8CqText ?? JS8_DEFAULT_CQ,
    replyText: s.js8ReplyText ?? JS8_DEFAULT_REPLY,
    savedMessages: s.js8SavedMessages ?? JS8_DEFAULT_SAVED,
    sendAtOnce: s.js8SendAtOnce !== false,
    pills: s.js8Pills !== false,
    wordSuggestions: s.js8WordSuggestions !== false,
    info: s.js8Info ?? '',
    status: s.js8Status ?? '',
  }
}

/** `addMessageText`: `insert` at the selection [selStart, selEnd) of `text`, after a space unless
 *  the caret starts the box or follows white space; with `clear`, in place of all of it. Answers the
 *  new text and where the caret goes (after what went in). */
export function js8InsertText(
  text: string,
  selStart: number,
  selEnd: number,
  insert: string,
  clear: boolean,
): { text: string; caret: number } {
  if (clear) return { text: insert, caret: insert.length }
  const a = Math.max(0, Math.min(selStart, selEnd, text.length))
  const b = Math.max(a, Math.min(Math.max(selStart, selEnd), text.length))
  const before = text.slice(0, a)
  const gap = before === '' || /\s$/.test(before) ? '' : ' '
  return { text: before + gap + insert + text.slice(b), caret: a + gap.length + insert.length }
}

/** The selected call's values: the To box's call when the station has heard it, its report, and
 *  the time delta of its newest decode still in the activity, in whole ms toward zero (JS8Call's
 *  `(int)(1000 * cd.tdrift)`). A `/P` decode is the same station as its base call. */
export function js8MacroSelected(
  toCall: string,
  stations: readonly Js8Heard[],
  activity: readonly Js8ActivityRow[],
): Js8MacroInput['selected'] {
  const call = toCall.trim().toUpperCase()
  const heard = call === '' ? undefined : stations.find((h) => h.call.toUpperCase() === call)
  if (!heard) return null
  const base = (c: string) => c.trim().toUpperCase().replace(/\/P$/, '')
  let newest: Js8ActivityRow | null = null
  for (const r of activity) {
    if (base(r.from) === base(call) && (newest === null || r.atMs >= newest.atMs)) newest = r
  }
  return {
    call: heard.call,
    snrDb: Math.round(heard.snrDb),
    tdeltaMs: newest ? Math.trunc(1000 * newest.dtS) : null,
  }
}

/** JS8Call's Settings refuse a CQ message that is not empty, does not start with CQ and does not
 *  hold the callsign (Configuration.cpp:3003-3008) — both read upper-cased, as its fields hold them. */
export function js8CqTextRefused(cqText: string, mycall: string): boolean {
  const cq = cqText.toUpperCase().trim()
  return cq !== '' && !(cq.startsWith('CQ') || cq.includes(mycall.toUpperCase().trim()))
}
