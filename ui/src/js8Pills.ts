// JS8Call 3.0's compose-box pills (DirectedMessageParser.cpp and PillRenderer.cpp read as the
// spec; "Display callsigns and commands as pills", on by default, Configuration.cpp:2187; user guide
// :249-251): the parts of a message JS8Call draws as coloured pills while it is typed — who it is
// from, who it goes to (a call, a relay route or a group) and the command — each with a short
// tooltip. Display only: nothing here decides what is sent; the station frames the text.
//
// The rules, in order, on the line upper-cased as JS8Call's editor has it:
//   • A line that starts with a backquote (a compound announcement) has no pills.
//   • `CALL: ` at the start, CALL a callsign that is not a group, is the SENDER.
//   • The ADDRESS at the start of what follows: a relay route `A>B>C` (spaces allowed around each
//     `>`) whose first hop is a callsign and whose later hops are standalone callsigns, counted up to
//     the first hop that is not — two or more is a RELAY route, one is the RECIPIENT; otherwise the
//     first word, an @GROUP that JS8Call's compound rule accepts (a GROUP), or a callsign (the
//     RECIPIENT); otherwise a route that starts with `>`, which continues from the call in the To
//     box when that is a callsign.
//   • With no address of its own, a line may start with a CQ or a heartbeat (`CQ CQ CQ EN52`,
//     `HB EN52`): that is a COMMAND pill, the square inside it when there is one.
//   • With an address, or a call in the To box, the first command after it is a COMMAND pill:
//     the longest command that matches there, ending on a space unless it ends in `?` or `:`,
//     with its argument where it takes one (`MSG TO:` a call, `QUERY CALL` a call, `QUERY MSG`
//     an id, `GRID` a locator) and the argument is good. A lone `?` is a command too.
// The token words are the mode's vocabulary (the invariant-token rule); the tooltips are
// catalogued in the component that draws the pills (Js8ComposeBox).

export type Js8PillKind = 'sender' | 'recipient' | 'relay' | 'group' | 'command'

/** The command pills' tooltip ids, one per meaning (the catalog's `js8.pill.cmd.<id>`). */
export type Js8PillCommand =
  | 'agnQ' | 'qslQ' | 'hwCpyQ' | 'snrQ' | 'infoQ' | 'gridQ' | 'statusQ' | 'queryMsgs' | 'hearingQ'
  | 'msgTo' | 'heartbeatSnr' | 'queryCall' | 'queryMsg' | 'query' | 'ditDit' | 'status' | 'hearing'
  | 'cmd' | 'msg' | 'nack' | 'ack' | 'snr' | 'qsl' | 'info' | 'grid' | 'seventyThree' | 'yes'
  | 'no' | 'rr' | 'sk' | 'fb' | 'cq' | 'cqContest' | 'cqField' | 'cqDx' | 'cqQrp' | 'hb' | 'other'

export interface Js8Pill {
  /** UTF-16 offsets into the text as typed. */
  start: number
  end: number
  kind: Js8PillKind
  /** sender / recipient / group: the call; relay: every hop, the last the recipient. */
  calls: string[]
  /** command: what it means. */
  command?: Js8PillCommand
  /** command: its argument when it takes one and the one typed is good, else null. */
  arg?: string | null
  /** command: the To box's call it goes to, when the line names no address of its own. */
  toBox?: string
}

type ArgKind = 'call' | 'queryCall' | 'id' | 'grid'
interface CommandDef {
  word: string
  command: Js8PillCommand
  arg?: ArgKind
  /** May start a line with no address (a CQ or a heartbeat). */
  bare?: boolean
}

// The commands JS8Call draws as pills, its own words. Matched longest first.
const COMMANDS: readonly CommandDef[] = [
  { word: 'AGN?', command: 'agnQ' },
  { word: 'QSL?', command: 'qslQ' },
  { word: 'HW CPY?', command: 'hwCpyQ' },
  { word: 'SNR?', command: 'snrQ' },
  { word: 'INFO?', command: 'infoQ' },
  { word: 'GRID?', command: 'gridQ' },
  { word: 'STATUS?', command: 'statusQ' },
  { word: 'QUERY MSGS?', command: 'queryMsgs' },
  { word: 'HEARING?', command: 'hearingQ' },
  { word: 'MSG TO:', command: 'msgTo', arg: 'call' },
  { word: 'HEARTBEAT SNR', command: 'heartbeatSnr' },
  { word: 'QUERY MSGS', command: 'queryMsgs' },
  { word: 'QUERY CALL', command: 'queryCall', arg: 'queryCall' },
  { word: 'QUERY MSG', command: 'queryMsg', arg: 'id' },
  { word: 'QUERY', command: 'query' },
  { word: 'DIT DIT', command: 'ditDit' },
  { word: 'STATUS', command: 'status' },
  { word: 'HEARING', command: 'hearing' },
  { word: 'CMD', command: 'cmd' },
  { word: 'MSG', command: 'msg' },
  { word: 'NACK', command: 'nack' },
  { word: 'ACK', command: 'ack' },
  { word: 'SNR', command: 'snr' },
  { word: 'QSL', command: 'qsl' },
  { word: 'INFO', command: 'info' },
  { word: 'GRID', command: 'grid', arg: 'grid' },
  { word: '73', command: 'seventyThree' },
  { word: 'YES', command: 'yes' },
  { word: 'NO', command: 'no' },
  { word: 'RR', command: 'rr' },
  { word: 'SK', command: 'sk' },
  { word: 'FB', command: 'fb' },
  { word: 'CQ CQ CQ', command: 'cq', arg: 'grid', bare: true },
  { word: 'CQ CONTEST', command: 'cqContest', arg: 'grid', bare: true },
  { word: 'CQ FIELD', command: 'cqField', arg: 'grid', bare: true },
  { word: 'CQ DX', command: 'cqDx', arg: 'grid', bare: true },
  { word: 'CQ QRP', command: 'cqQrp', arg: 'grid', bare: true },
  { word: 'CQ FD', command: 'cqField', arg: 'grid', bare: true },
  { word: 'CQ CQ', command: 'cq', arg: 'grid', bare: true },
  { word: 'CQ', command: 'cq', arg: 'grid', bare: true },
  { word: 'HB', command: 'hb', arg: 'grid', bare: true },
  { word: 'HEARTBEAT', command: 'hb', arg: 'grid', bare: true },
]
const LONGEST_FIRST = [...COMMANDS].sort((a, b) => b.word.length - a.word.length || (a.word < b.word ? -1 : a.word > b.word ? 1 : 0))

// ── JS8Call's callsign rules, as crates/js8/src/proto/callsign.rs ports them ────────────────

/** The specials JS8Call's `basecalls` table holds besides a call: the compound placeholder and
 *  the @ names (varicode.cpp's `basecalls`; js8::proto::alphabet::GROUPS). */
const SPECIALS: ReadonlySet<string> = new Set([
  '<....>', '@ALLCALL', '@JS8NET',
  '@DX/NA', '@DX/SA', '@DX/EU', '@DX/AS', '@DX/AF', '@DX/OC', '@DX/AN', '@REGION/1', '@REGION/2',
  '@REGION/3', '@GROUP/0', '@GROUP/1', '@GROUP/2', '@GROUP/3', '@GROUP/4', '@GROUP/5', '@GROUP/6',
  '@GROUP/7', '@GROUP/8', '@GROUP/9', '@COMMAND', '@CONTROL', '@NET', '@NTS', '@RESERVE/0',
  '@RESERVE/1', '@RESERVE/2', '@RESERVE/3', '@RESERVE/4', '@APRSIS', '@RAGCHEW', '@JS8', '@EMCOMM',
  '@ARES', '@MARS', '@AMRRON', '@RACES', '@RAYNET', '@RADAR', '@SKYWARN', '@CQ', '@HB', '@QSO',
  '@QSOPARTY', '@CONTEST', '@FIELDDAY', '@SOTA', '@IOTA', '@POTA', '@QRP', '@QRO',
])

const isDigit = (c: string) => c >= '0' && c <= '9'
const isUpper = (c: string) => c >= 'A' && c <= 'Z'
const hasPair = (s: string) => {
  for (let i = 0; i + 1 < s.length; i++) {
    if ((isDigit(s[i]) && isUpper(s[i + 1])) || (isUpper(s[i]) && isDigit(s[i + 1]))) return true
  }
  return false
}

/** The base call's shape — one or two leading characters, a digit, up to three letters — and a
 *  digit-letter pair, 3 to 6 characters. */
function shapeAndPair(core: string): boolean {
  const n = core.length
  if (n < 3 || n > 6) return false
  const fits = (prefix: number) =>
    n > prefix &&
    [...core.slice(0, prefix)].every((c) => isDigit(c) || isUpper(c)) &&
    isDigit(core[prefix]) &&
    [...core.slice(prefix + 1)].every(isUpper) &&
    n - prefix - 1 <= 3
  return (fits(1) || fits(2)) && hasPair(core)
}

/** A base call, with JS8Call's 3DA0 and 3X prefix workarounds. */
function isBaseCall(s: string): boolean {
  let c = s
  if (c.startsWith('3DA0')) c = `3D0${c.slice(4)}`
  if (c.length > 2 && c.startsWith('3X') && isUpper(c[2])) c = `Q${c.slice(2)}`
  return shapeAndPair(c)
}

/** JS8Call's `isCompoundCallsign`. */
function isCompoundCall(s: string): boolean {
  const n = s.length
  if (n === 0 || n > 11) return false
  if (SPECIALS.has(s) && !s.startsWith('@')) return false
  if (shapeAndPair(s.endsWith('/P') ? s.slice(0, -2) : s)) return false
  for (let i = 0; i < n; i++) {
    const c = s[i]
    if (!(isDigit(c) || isUpper(c) || c === '/' || (i === 0 && c === '@'))) return false
  }
  if (n - s.split('/').length + 1 > 9) return false
  const slash = s.indexOf('/')
  if (slash >= 0) return !SPECIALS.has(s.slice(0, slash))
  if (s.startsWith('@')) return true
  return n > 2 && hasPair(s)
}

/** JS8Call's `isValidCallsign`: a special, a base call with or without `/P`, or a compound call. */
export function isJs8Callsign(s: string): boolean {
  return SPECIALS.has(s) || isBaseCall(s.endsWith('/P') ? s.slice(0, -2) : s) || isCompoundCall(s)
}

const CALL_WORD = /^@?[A-Z0-9/]+$/
/** A hop after the first in a relay route: a standalone callsign, prefix and suffix allowed. */
const FOLLOW_HOP = /^(?:[A-Z0-9]{1,4}\/)?[0-9A-Z]?[0-9A-Z][0-9][A-Z]?[A-Z]?[A-Z]?(?:\/[A-Z0-9]{1,4})?$/
const GRID = /^[A-R]{2}[0-9]{2}(?:[A-X]{2}(?:[0-9]{2}(?:[A-X]{2}(?:[0-9]{2})?)?)?)?$/

function argOf(kind: ArgKind, token: string): string | null {
  switch (kind) {
    case 'call':
      return isJs8Callsign(token) ? token : null
    case 'queryCall': {
      const call = token.endsWith('?') ? token.slice(0, -1) : token
      return call && isJs8Callsign(call) ? call : null
    }
    case 'id':
      return /^[0-9]+$/.test(token) ? token : null
    case 'grid':
      return GRID.test(token) ? token : null
  }
}

interface CommandMatch {
  start: number
  end: number
  command: Js8PillCommand
  arg: string | null
}

function matchCommandAt(text: string, start: number, bareOnly: boolean): CommandMatch | null {
  if (start < 0 || start >= text.length) return null
  for (const def of LONGEST_FIRST) {
    if (!text.startsWith(def.word, start)) continue
    const after = start + def.word.length
    const last = def.word[def.word.length - 1]
    if (last !== '?' && last !== ':' && after < text.length && text[after] !== ' ') continue
    if (bareOnly && !def.bare) return null
    const m: CommandMatch = { start, end: after, command: def.command, arg: null }
    if (def.arg) {
      let argStart = after
      while (argStart < text.length && text[argStart] === ' ') argStart++
      if (argStart < text.length) {
        const sp = text.indexOf(' ', argStart)
        const argEnd = sp < 0 ? text.length : sp
        const arg = argOf(def.arg, text.slice(argStart, argEnd))
        if (arg !== null) {
          m.arg = arg
          m.end = argEnd
        }
      }
    }
    return m
  }
  if (!bareOnly && text[start] === '?') return { start, end: start + 1, command: 'other', arg: null }
  return null
}

/** The hops of a route: each run up to a `>` or a space. */
function hopsOf(chain: string): [number, number][] {
  const hops: [number, number][] = []
  let i = 0
  while (i < chain.length) {
    while (i < chain.length && (chain[i] === ' ' || chain[i] === '>')) i++
    const from = i
    while (i < chain.length && chain[i] !== '>' && chain[i] !== ' ') i++
    if (i > from) hops.push([from, i])
  }
  return hops
}

const firstHopOk = (h: string) => !h.startsWith('@') && isJs8Callsign(h)
const laterHopOk = (h: string) => !h.startsWith('@') && FOLLOW_HOP.test(h)

/** The good hops of a route, counted to the first that is not; `partial` continues from `toBox`. */
function goodHops(chain: string, partial: boolean, toBox: string): [number, number][] {
  const hops = hopsOf(chain)
  if (hops.length === 0 || (partial && !firstHopOk(toBox))) return []
  const good: [number, number][] = []
  for (const [i, [a, b]] of hops.entries()) {
    const h = chain.slice(a, b)
    if (!(partial || i > 0 ? laterHopOk(h) : firstHopOk(h))) break
    good.push([a, b])
  }
  return good
}

/** Upper-case each UTF-16 unit that stays one unit (`ß` stays `ß`), so offsets hold. */
function upper(text: string): string {
  let out = ''
  for (const c of text) {
    const u = c.toUpperCase()
    out += u.length === c.length ? u : c
  }
  return out
}

/** The pills JS8Call draws for one line of the compose box, with `toBox` the selected call. */
export function js8Pills(typed: string, toBox: string): Js8Pill[] {
  const text = upper(typed)
  const target = upper(toBox.trim())
  const pills: Js8Pill[] = []
  if (text === '' || text.startsWith('`')) return pills
  let offset = 0
  const colon = text.indexOf(':')
  if (colon > 0 && colon + 1 < text.length && text[colon + 1] === ' ') {
    const sender = text.slice(0, colon)
    if (CALL_WORD.test(sender) && !sender.startsWith('@') && isJs8Callsign(sender)) {
      pills.push({ start: 0, end: colon + 1, kind: 'sender', calls: [sender] })
      offset = colon + 2
    }
  }
  const content = text.slice(offset)
  const at = (p: Js8Pill): Js8Pill => ({ ...p, start: p.start + offset, end: p.end + offset })
  let pos = 0
  let addressed = false
  const route = /^@?[A-Z0-9/]+(?:\s*>\s*@?[A-Z0-9/]+)+/.exec(content)
  if (route) {
    const good = goodHops(route[0], false, '')
    if (good.length >= 2) {
      const end = good[good.length - 1][1]
      pills.push(at({ start: 0, end, kind: 'relay', calls: good.map(([a, b]) => content.slice(a, b)) }))
      addressed = true
      pos = end
    } else if (good.length === 1) {
      const [a, b] = good[0]
      pills.push(at({ start: a, end: b, kind: 'recipient', calls: [content.slice(a, b)] }))
      addressed = true
      pos = b
    }
  }
  if (!addressed) {
    const sp = content.indexOf(' ')
    const word = sp < 0 ? content : content.slice(0, sp)
    if (word.startsWith('@')) {
      if (isCompoundCall(word)) {
        pills.push(at({ start: 0, end: word.length, kind: 'group', calls: [word] }))
        addressed = true
        pos = word.length
      }
    } else if (word && CALL_WORD.test(word) && isJs8Callsign(word)) {
      pills.push(at({ start: 0, end: word.length, kind: 'recipient', calls: [word] }))
      addressed = true
      pos = word.length
    }
    if (!addressed && content.startsWith('>')) {
      const partial = /^(?:\s*>\s*@?[A-Z0-9/]+)+/.exec(content)
      const good = partial ? goodHops(partial[0], true, target) : []
      if (good.length >= 1) {
        const end = good[good.length - 1][1]
        pills.push(at({ start: 0, end, kind: 'relay', calls: good.map(([a, b]) => content.slice(a, b)) }))
        addressed = true
        pos = end
      }
    }
  }
  if (!addressed) {
    const bare = matchCommandAt(content, 0, true)
    if (bare) {
      pills.push(at({ start: bare.start, end: bare.end, kind: 'command', calls: [], command: bare.command, arg: bare.arg }))
      return pills
    }
  }
  if ((addressed || target !== '') && pos < content.length) {
    while (pos < content.length && content[pos] === ' ') pos++
    const m = matchCommandAt(content, pos, false)
    if (m) {
      pills.push(
        at({
          start: m.start,
          end: m.end,
          kind: 'command',
          calls: [],
          command: m.command,
          arg: m.arg,
          ...(!addressed && target !== '' ? { toBox: target } : {}),
        }),
      )
    }
  }
  return pills
}
