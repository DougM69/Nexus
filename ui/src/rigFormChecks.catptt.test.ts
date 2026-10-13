// The radios Hamlib cannot key over CAT, as the rig form and the setup wizard know them.
//
// PTT on CAT keys the radio with rigctld's `T 1`. For thirteen radios in the catalog the bundled
// Hamlib's caps say `Can set PTT: N`, and on the IC-706MKIIG `T 1` still answers RPRT 0 while the
// radio stays on receive, so Nexus showed TX and nothing went out. `NO_CAT_PTT_MODELS` is how the
// wizard stays off CAT for them and how Settings warns, and it is pinned here to the caps the
// bundled Hamlib dumps (`scripts/gen-hamlib-cat-ptt.mjs` writes `hamlibCatPtt.json`): a row typed
// by hand, or a radio dropped from the list, fails here instead of on the air.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  blocks,
  checkRigForm,
  NO_CAT_PTT_MODELS,
  NO_CAT_PTT_TRANSCEIVERS,
  type RigFormFacts,
} from './rigFormChecks'
import hamlibCatPtt from './components/__fixtures__/hamlibCatPtt.json'

describe("the list is the bundled Hamlib's answer, not a hand list", () => {
  it('the fixture is the bundled Hamlib and covers the catalog', () => {
    // A different build here means the facts below are about a different Hamlib than the
    // operator's copy: regenerate with `node scripts/gen-hamlib-cat-ptt.mjs`.
    expect(hamlibCatPtt.hamlib).toMatch(/^Hamlib 4\.7\.1 /)
    // A fixture cannot vouch for its own completeness; only the catalog can. A rig added to the
    // catalog since the last regeneration is missing here, and so missing from every check below.
    const catalog = readFileSync(
      fileURLToPath(new URL('../../crates/tempo-audio/src/rigmodels.rs', import.meta.url)),
      'utf8',
    )
    const inCatalog = [...new Set([...catalog.matchAll(/^\s*\(\s*(\d+)\s*,/gm)].map((m) => Number(m[1])))]
    // The parse is this test's own positive control: a pattern that matched nothing would leave
    // nothing unaccounted for, on no evidence at all.
    expect(inCatalog.length).toBeGreaterThan(100)
    const accountedFor = new Set<number>([
      ...hamlibCatPtt.rigs.map((r) => r.model),
      ...hamlibCatPtt.missing,
      ...hamlibCatPtt.needsDaemon,
    ])
    const unaccounted = inCatalog.filter((m) => !accountedFor.has(m)).sort((a, b) => a - b)
    expect({ unaccounted }).toEqual({ unaccounted: [] })
    // Only NET rigctl (2) cannot dump its caps without a live rigctld. Any other model here is a
    // dump that failed, not a fact: rigctl.exe under WSL interop has dropped four models from one
    // run and dumped all four on the next, so regenerate rather than trust a missing row.
    expect(hamlibCatPtt.needsDaemon).toEqual([2])
    expect(hamlibCatPtt.missing).toEqual([])
  })

  it('NO_CAT_PTT_MODELS is exactly the rigs whose caps say Can set PTT: N', () => {
    const cannot = hamlibCatPtt.rigs.filter((r) => !r.setPtt).map((r) => r.model)
    // Control: the fixture really holds such rows, so the equality below is not [] against [].
    expect(cannot.length).toBeGreaterThan(0)
    expect([...NO_CAT_PTT_MODELS].sort((a, b) => a - b)).toEqual(cannot.sort((a, b) => a - b))
  })

  it('NO_CAT_PTT_TRANSCEIVERS is exactly the rigs that cannot set PTT and can transmit', () => {
    // The radios the wizard asks how the interface keys them, and Settings explains under PTT
    // Method. A receiver among the `Can set PTT: N` rows has nothing to key: the Malachite DSP's
    // caps say `Rig type: Other Receiver`, with no Transmitter in the line.
    expect(hamlibCatPtt.rigs.every((r) => typeof r.transmitter === 'boolean')).toBe(true)
    const asks = hamlibCatPtt.rigs.filter((r) => !r.setPtt && r.transmitter).map((r) => r.model)
    const receivers = hamlibCatPtt.rigs.filter((r) => !r.setPtt && !r.transmitter).map((r) => r.model)
    // Controls: both kinds are in the fixture, so the equality below is not [] against [], and the
    // transmitter fact really takes a radio off NO_CAT_PTT_MODELS.
    expect(asks.length).toBeGreaterThan(0)
    expect(receivers.length).toBeGreaterThan(0)
    expect([...NO_CAT_PTT_TRANSCEIVERS].sort((a, b) => a - b)).toEqual(asks.sort((a, b) => a - b))
  })
})

describe('Settings says so when PTT is on CAT for one of them', () => {
  const HINT = /^Hamlib can't key this radio over CAT, so PTT on CAT won't put it in transmit\. /
  const ic706 = (over: Partial<RigFormFacts> = {}): RigFormFacts => ({
    serialPort: 'COM5',
    rigConn: 'serial',
    pttMethod: 'cat',
    rigModel: 3011,
    ...over,
  })
  const check = (form: RigFormFacts) => checkRigForm(form, ['COM5'], [0, 1, 2, 3, 4])

  it('IC-706MKIIG on CAT: one warning, never an error, so the save goes ahead', () => {
    // A SignaLink keys the radio on audio, so CAT "works" for it: the operator must keep saving.
    const checks = check(ic706())
    expect(checks).toEqual([{ level: 'warning', message: expect.stringMatching(HINT) }])
    expect(blocks(checks)).toBe(false)
  })

  it('IC-706MKIIG on VOX: nothing to say', () => {
    expect(check(ic706({ pttMethod: 'vox' }))).toEqual([])
  })

  it('IC-7300 on CAT: nothing to say, Hamlib keys it', () => {
    expect(check(ic706({ rigModel: 3073 }))).toEqual([])
  })

  it('over a network address, and on the empty connection an older settings file loads as', () => {
    // rigctld drives the same model either way, and the backend reads an empty connection as serial.
    expect(check(ic706({ rigConn: 'network', serialPort: '' }))).toEqual([
      { level: 'warning', message: expect.stringMatching(HINT) },
    ])
    expect(check(ic706({ rigConn: '' }))).toEqual([
      { level: 'warning', message: expect.stringMatching(HINT) },
    ])
  })

  it('not on OmniRig, whose own rig file keys the radio', () => {
    expect(check(ic706({ rigConn: 'omnirig', serialPort: '' }))).toEqual([])
  })
})
