// The RTTY Auto-tune pair finder, by value, on synthetic spectra drawn the way the producer draws
// them: 512 display bins over 0–4000 Hz, each a 0..1 value on the WF_DB_SPAN dB axis.
import { describe, it, expect } from 'vitest'
import { AUTO_TUNE_SEARCH_HZ, findTonePair } from './rttyAutoTune'
import { WF_DB_SPAN } from '../waterfall'

const LO = 0
const HI = 4000
const N = 512
const W = (HI - LO) / N
const FLOOR_DB = -95

/** dB → the display value the producer sends. */
const disp = (db: number) => Math.min(1, Math.max(0, 1 + db / WF_DB_SPAN))

/** A row of noise (deterministic ±1.5 dB ripple) with a Hann-ish lobe at each tone. */
function row(tones: { hz: number; db: number }[], seed = 1): number[] {
  const db = Array.from({ length: N }, (_, i) => FLOOR_DB + 1.5 * Math.sin(i * 1.7 + seed * 3.1))
  for (const { hz, db: level } of tones) {
    const at = (hz - LO) / W - 0.5 // fractional bin of the tone
    for (let i = Math.floor(at) - 3; i <= Math.ceil(at) + 3; i++) {
      if (i < 0 || i >= N) continue
      const d = Math.abs(i - at)
      // ~3 bins of main lobe (23 Hz at 7.8 Hz/bin), 12 dB down a bin off the peak.
      db[i] = Math.max(db[i], level - 12 * d * d)
    }
  }
  return db.map(disp)
}

// The scene every case starts from: the wanted 170 Hz pair centred on 1500, a lone carrier, and a
// pair at the wrong shift (425 Hz) — all strong, all near.
const PAIR = [{ hz: 1415, db: -40 }, { hz: 1585, db: -42 }]
const DECOY_TONE = { hz: 1300, db: -35 }
const DECOY_PAIR = [{ hz: 1650, db: -38 }, { hz: 2075, db: -38 }]

describe('findTonePair', () => {
  it('nets onto the pair at the configured shift, ignoring a lone tone and a pair at another shift', () => {
    // Started from 1700: the wrong-shift pair's centre (1862.5) is nearer than the wanted one's.
    const r = findTonePair([row([...PAIR, DECOY_TONE, ...DECOY_PAIR])], LO, HI, 170, 1700)
    expect(r).not.toBeNull()
    expect(r!.lowHz).toBeCloseTo(1415, -1)
    expect(r!.highHz).toBeCloseTo(1585, -1)
    expect(Math.abs(r!.centerHz - 1500)).toBeLessThan(4)
  })

  it('the shift is what decides: at 425 Hz it takes the other pair', () => {
    const r = findTonePair([row([...PAIR, DECOY_TONE, ...DECOY_PAIR])], LO, HI, 425, 1700)
    expect(r).not.toBeNull()
    expect(Math.abs(r!.centerHz - 1862.5)).toBeLessThan(4)
  })

  it('of two pairs at the shift, takes the one nearest the current tones', () => {
    // Both within reach of either start; the second is the stronger.
    const other = [{ hz: 1665, db: -30 }, { hz: 1835, db: -30 }]
    const r = findTonePair([row([...PAIR, ...other])], LO, HI, 170, 1550)
    expect(Math.abs(r!.centerHz - 1500)).toBeLessThan(4)
    // …and with the tones started nearer the other pair, that one.
    const r2 = findTonePair([row([...PAIR, ...other])], LO, HI, 170, 1720)
    expect(Math.abs(r2!.centerHz - 1750)).toBeLessThan(4)
  })

  it('looks only near the current tones: a pair beyond the search radius is not found', () => {
    expect(findTonePair([row(PAIR)], LO, HI, 170, 2500)).toBeNull()
    // Just past the radius: both tones are in view of the search, the centre is 305 Hz off.
    expect(findTonePair([row(PAIR)], LO, HI, 170, 1500 - AUTO_TUNE_SEARCH_HZ - 5)).toBeNull()
    // Positive controls: the same row, started near it, and just inside the radius.
    expect(findTonePair([row(PAIR)], LO, HI, 170, 1600)).not.toBeNull()
    expect(findTonePair([row(PAIR)], LO, HI, 170, 1500 - AUTO_TUNE_SEARCH_HZ + 5)).not.toBeNull()
  })

  it('finds nothing in noise alone, nor in a lone tone', () => {
    expect(findTonePair([row([])], LO, HI, 170, 1500)).toBeNull()
    expect(findTonePair([row([DECOY_TONE])], LO, HI, 170, 1300)).toBeNull()
  })

  it('a pair too weak to stand over the noise is not a pair', () => {
    const weak = [{ hz: 1415, db: FLOOR_DB + 4 }, { hz: 1585, db: FLOOR_DB + 4 }]
    expect(findTonePair([row(weak)], LO, HI, 170, 1500)).toBeNull()
  })

  it('averages its rows: mark in one and space in the next still make the pair', () => {
    // A keyed RTTY signal sits on one tone at a time; a short window can catch only one.
    const rows = [row([PAIR[0]], 1), row([PAIR[1]], 2), row([PAIR[0]], 3), row([PAIR[1]], 4)]
    const r = findTonePair(rows, LO, HI, 170, 1500)
    expect(r).not.toBeNull()
    expect(Math.abs(r!.centerHz - 1500)).toBeLessThan(4)
    // Control: either row alone holds one tone and no pair.
    expect(findTonePair([rows[0]], LO, HI, 170, 1500)).toBeNull()
  })

  it('refuses an empty or degenerate input rather than guessing', () => {
    expect(findTonePair([], LO, HI, 170, 1500)).toBeNull()
    expect(findTonePair([row(PAIR)], 4000, 0, 170, 1500)).toBeNull()
    expect(findTonePair([row(PAIR)], LO, HI, 0, 1500)).toBeNull()
  })
})
