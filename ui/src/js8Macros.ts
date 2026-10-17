// JS8Call's 13 message macros for the compose box — the desktop half of `js8::proto::macros`
// (crates/js8/src/proto/macros.rs, whose header is the contract). JS8Call replaces them with the
// station's own values whenever it makes a message (`buildMacroValues`, mainwindow.cpp:4848-4888;
// `replaceMacros`, mainwindow.h:789-799): a button or a saved message FILLS the compose box with
// every `<…>` it cannot value pruned, and a send replaces them WITHOUT pruning
// (`createMessage`, mainwindow.cpp:3436-3440). The station does the same for the texts it makes
// itself (the CQ, the INFO? and STATUS? replies). Both halves are held to one table of cases,
// crates/js8/tests/fixtures/js8-macros.json (js8Macros.agreement.test.ts).
//
// The tokens and the version name are the mode's vocabulary, never prose (the invariant-token rule).

/** The 13 tokens, in JS8Call's map order (sorted, as its `QMap` keeps them). */
export const JS8_MACRO_TOKENS = [
  '<CALL>',
  '<MYCALL>',
  '<MYCQ>',
  '<MYGRID12>',
  '<MYGRID4>',
  '<MYHB>',
  '<MYIDLE>',
  '<MYINFO>',
  '<MYREPLY>',
  '<MYSTATUS>',
  '<MYVERSION>',
  '<SNR>',
  '<TDELTA>',
] as const

/** The square: the macro JS8Call's own CQ forms end with. */
export const JS8_MYGRID4 = '<MYGRID4>'
/** JS8Call's CQ message (`CQMessage`). */
export const JS8_DEFAULT_CQ = 'CQ CQ CQ <MYGRID4>'
/** JS8Call's reply message (`Reply`). */
export const JS8_DEFAULT_REPLY = 'HW CPY?'
/** JS8Call's STATUS (`MyStatus`): what an empty STATUS stands for. */
export const JS8_DEFAULT_STATUS = 'IDLE <MYIDLE> VERSION <MYVERSION>'
/** JS8Call's one saved message out of the box (`Macros`). */
export const JS8_DEFAULT_SAVED: readonly string[] = ['TNX 73 GL']
/** JS8Call's heartbeat text; its editor is compiled out upstream, so `<MYHB>` is always this. */
const HB_TEXT = 'HB <MYGRID4>'
/** What `<MYVERSION>` names: the application, as the station's STATUS reply says it. */
const VERSION = 'Nexus'

/** What the values are made from: the settings, the idle count and the selected call. */
export interface Js8MacroInput {
  mycall: string
  grid: string
  info: string
  /** STATUS as set; empty stands for JS8_DEFAULT_STATUS. */
  status: string
  cq: string
  reply: string
  /** Whole minutes since the operator last acted. */
  idleMinutes: number
  /** The call in the To box when the station has heard it (`m_callActivity.contains`), with its
   *  report and `(int)(1000 * time drift)` of its newest decode — null when no decode of it is
   *  still in the activity, and `<TDELTA>` then has no value. */
  selected: { call: string; snrDb: number; tdeltaMs: number | null } | null
}

export type Js8MacroValues = ReadonlyArray<readonly [string, string]>

/** JS8Call's `Varicode::formatSNR`: a sign and two digits, and nothing past ±60. */
export function js8FormatSnr(snr: number): string {
  if (snr < -60 || snr > 60) return ''
  return `${snr < 0 ? '-' : '+'}${String(Math.abs(snr)).padStart(2, '0')}`
}

/** `since()` of the last activity for a whole-minute idle count, upper-cased, "NOW" as "0M". */
export function js8IdleSince(minutes: number): string {
  const secs = Math.max(0, Math.trunc(minutes)) * 60
  if (secs >= 86_400) return `${Math.trunc(secs / 86_400)}D`
  if (secs >= 3_600) return `${Math.trunc(secs / 3_600)}H`
  if (secs >= 60) return `${Math.trunc(secs / 60)}M`
  return '0M'
}

/** `buildMacroValues`: the pairs in token order, the five texts run once through them. */
export function js8MacroValues(input: Js8MacroInput): Js8MacroValues {
  const grid = (n: number) => [...input.grid].slice(0, n).join('')
  const status = input.status.trim() || JS8_DEFAULT_STATUS
  const pairs: [string, string][] = [
    ['<MYCALL>', input.mycall],
    ['<MYCQ>', input.cq.trim().split('CQCQCQ').join('CQ CQ CQ')],
    ['<MYGRID12>', grid(12)],
    ['<MYGRID4>', grid(4)],
    ['<MYHB>', HB_TEXT],
    ['<MYIDLE>', js8IdleSince(input.idleMinutes)],
    ['<MYINFO>', input.info.trim()],
    ['<MYREPLY>', input.reply.trim()],
    ['<MYSTATUS>', status],
    ['<MYVERSION>', VERSION],
  ]
  const sel = input.selected
  if (sel) {
    pairs.push(['<CALL>', sel.call])
    if (sel.tdeltaMs !== null) pairs.push(['<TDELTA>', `${sel.tdeltaMs} ms`])
    if (sel.snrDb > -31) pairs.push(['<SNR>', js8FormatSnr(sel.snrDb)])
  }
  pairs.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
  for (const token of ['<MYINFO>', '<MYSTATUS>', '<MYCQ>', '<MYHB>', '<MYREPLY>']) {
    const slot = pairs.find((p) => p[0] === token)
    if (slot) slot[1] = replaceJs8Macros(slot[1], pairs, false)
  }
  return pairs
}

/** Every `token` in `text`, found left to right whatever its ASCII case, replaced by `value`.
 *  Folding only ASCII letters keeps every offset, so a match in the folded copy is one in `text`. */
function replaceIgnoringAsciiCase(text: string, token: string, value: string): string {
  const folded = text.replace(/[a-z]/g, (c) => c.toUpperCase())
  let out = ''
  let last = 0
  for (let at = folded.indexOf(token); at >= 0; at = folded.indexOf(token, at + token.length)) {
    out += text.slice(last, at) + value
    last = at + token.length
  }
  return out + text.slice(last)
}

/** `replaceMacros`: each token in turn, its value upper-cased; then with `prune` every `<…>` left
 *  removed (`[<](?:[^>]+)[>]`). */
export function replaceJs8Macros(text: string, values: Js8MacroValues, prune: boolean): string {
  let out = text
  for (const [token, value] of values) out = replaceIgnoringAsciiCase(out, token, value.toUpperCase())
  return prune ? out.replace(/<[^>]+>/g, '') : out
}
