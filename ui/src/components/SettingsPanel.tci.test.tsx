// @vitest-environment jsdom
//
// The TCI connection's per-radio fields travel in the radio patch: the seam that has dropped
// per-radio fields on Save before (the Flex three, `omnirigSlot`, `icomDataMode`). Every save of the
// rig form for a radio not in use goes `updateRadioProfile(id, radioPatch(form))`, so a field
// `radioPatch` leaves out is one the save never writes. Rust takes each of the seven as optional
// and leaves a stored value alone when it is absent; the form states every one.
import { describe, it, expect, vi } from 'vitest'
import { radioPatch } from './SettingsPanel'

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? vi.fn(() => Promise.resolve(null)) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const TCI_KEYS = ['tciHost', 'tciPort', 'tciTrx', 'tciDialect', 'tciAudio', 'tciPan', 'tciSpots'] as const

/** The seven TCI fields of a patch. */
const tciOf = (p: ReturnType<typeof radioPatch>) => Object.fromEntries(TCI_KEYS.map((k) => [k, p[k]]))

describe('the TCI connection travels with the radio it belongs to', () => {
  it('radioPatch carries every TCI field the form holds, each away from its default', () => {
    const stated = {
      tciHost: '192.0.2.40',
      tciPort: 40001,
      tciTrx: 1,
      tciDialect: 'pihpsdr',
      tciAudio: false,
      tciPan: true,
      tciSpots: true,
    }
    expect(tciOf(radioPatch({ rigConn: 'tci', ...stated }))).toEqual(stated)
  })

  it("a form that never held them sends Rust's defaults: this computer on 50001, audio on", () => {
    expect(tciOf(radioPatch({}))).toEqual({
      tciHost: '127.0.0.1',
      tciPort: 50001,
      tciTrx: 0,
      tciDialect: 'auto',
      tciAudio: true,
      tciPan: false,
      tciSpots: false,
    })
  })
})
