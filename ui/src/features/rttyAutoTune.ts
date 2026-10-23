// RTTY AUTO-TUNE — find the RTTY signal nearest the decoder's tones and say where its centre is.
// The pure half: spectrum rows in, a mark/space pair (or nothing) out. The cockpit samples the
// rows, refuses while we are keyed, and nets onto the answer through `rtty_net`.
//
// ⚠️ NETTING MOVES THE TRANSMITTER. The AFSK tones are built from the netted centre (since 1.9.0),
// so what this returns becomes our transmit frequency. That is why it is conservative: it answers
// only for two tones standing clearly over the noise, spaced at the configured shift, centred
// within `AUTO_TUNE_SEARCH_HZ` of where the decoder already is — and null rather than a guess.
//
// WHAT A ROW IS: the producer's display row (`tempo_core::spectrum`), each bin the PEAK raw power
// in its span on the 0..1 dB axis (`WF_DB_SPAN` dB, 1 = full scale). Rows are averaged in POWER
// (a mean of the display values would be a geometric mean of powers), because a keyed RTTY signal
// sits on one tone at a time and a single short window can catch only one of the pair.
import { WF_DB_SPAN } from '../waterfall'

/** How far from the decoder's current centre a pair's centre may sit (Hz). Wide enough for an
 *  operator who parked near a signal, narrow enough that the button never jumps the band. */
export const AUTO_TUNE_SEARCH_HZ = 300
/** A tone must stand this far over the row's median (dB) to count. */
export const AUTO_TUNE_PEAK_DB = 10
/** A peak is the highest bin within this many bins either side (a Hann lobe spans ~3). */
const PEAK_HALF_BINS = 2

export interface TonePair {
  lowHz: number
  highHz: number
  /** The midpoint — what `rtty_net` takes. */
  centerHz: number
}

interface Peak {
  hz: number
  db: number
}

/** The rows' power average, in dB. Rows of differing length are a caller error: null. */
function averageDb(rows: ArrayLike<number>[]): number[] | null {
  const n = rows[0]?.length ?? 0
  if (n < 2 * PEAK_HALF_BINS + 1 || rows.some((r) => r.length !== n)) return null
  const acc = new Array<number>(n).fill(0)
  for (const r of rows) for (let i = 0; i < n; i++) acc[i] += 10 ** (((r[i] - 1) * WF_DB_SPAN) / 10)
  return acc.map((p) => 10 * Math.log10(Math.max(p / rows.length, 1e-30)))
}

function median(v: number[]): number {
  const s = [...v].sort((a, b) => a - b)
  return s[Math.floor(s.length / 2)]
}

/** Local maxima over the floor, refined to a fraction of a bin by a parabola through the peak. */
function peaks(db: number[], loHz: number, w: number, from: number, to: number): Peak[] {
  const floor = median(db)
  const out: Peak[] = []
  for (let i = Math.max(PEAK_HALF_BINS, from); i <= Math.min(db.length - 1 - PEAK_HALF_BINS, to); i++) {
    const v = db[i]
    if (v < floor + AUTO_TUNE_PEAK_DB) continue
    let top = true
    // Strict on the left, so a flat-topped lobe yields one peak, not two.
    for (let k = 1; k <= PEAK_HALF_BINS && top; k++) top = v > db[i - k] && v >= db[i + k]
    if (!top) continue
    const a = db[i - 1]
    const c = db[i + 1]
    const den = a - 2 * v + c
    const d = den < 0 ? Math.max(-0.5, Math.min(0.5, (0.5 * (a - c)) / den)) : 0
    out.push({ hz: loHz + (i + 0.5 + d) * w, db: v })
  }
  return out
}

/**
 * The tone pair spaced `shiftHz` apart whose centre is nearest `nearHz`, from the power average
 * of `rows` spanning `loHz..hiHz`; null when there is none within `AUTO_TUNE_SEARCH_HZ`. Of two
 * pairs equally near (within a bin), the one whose weaker tone is stronger.
 */
export function findTonePair(
  rows: ArrayLike<number>[],
  loHz: number,
  hiHz: number,
  shiftHz: number,
  nearHz: number,
): TonePair | null {
  if (rows.length === 0 || !(hiHz > loHz) || !(shiftHz > 0)) return null
  const db = averageDb(rows)
  if (!db) return null
  const w = (hiHz - loHz) / db.length
  // A tone of a qualifying pair lies within the search radius plus half the shift (and a bin).
  const reach = AUTO_TUNE_SEARCH_HZ + shiftHz / 2 + w
  const found = peaks(db, loHz, w, Math.floor((nearHz - reach - loHz) / w), Math.ceil((nearHz + reach - loHz) / w))
  // The parabola is good to well under a bin; the shift itself is the operator's setting, so
  // allow a bin and a half or 8 % of it, whichever is wider.
  const tol = Math.max(1.5 * w, 0.08 * shiftHz)
  let best: (TonePair & { dist: number; weak: number }) | null = null
  for (let i = 0; i < found.length; i++) {
    for (let j = i + 1; j < found.length; j++) {
      const lo = found[i]
      const hi = found[j]
      if (Math.abs(hi.hz - lo.hz - shiftHz) > tol) continue
      const centerHz = (lo.hz + hi.hz) / 2
      const dist = Math.abs(centerHz - nearHz)
      if (dist > AUTO_TUNE_SEARCH_HZ) continue
      const weak = Math.min(lo.db, hi.db)
      if (!best || dist < best.dist - w || (Math.abs(dist - best.dist) <= w && weak > best.weak)) {
        best = { lowHz: lo.hz, highHz: hi.hz, centerHz, dist, weak }
      }
    }
  }
  return best && { lowHz: best.lowHz, highHz: best.highHz, centerHz: best.centerHz }
}
