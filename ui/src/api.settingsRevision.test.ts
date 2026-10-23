// @vitest-environment jsdom
// On a computer driving a station, each settings read comes with the station's revision of what it
// read (client mode puts it beside the settings). `api.ts` takes it off and keeps it on the copy,
// where no field or JSON sees it but every copy spread from that read carries it, so a save made
// from the read goes back with its revision, whatever this window read since: a change made at the
// station after that read refuses the save, as a stale save must be refused.
import { afterEach, describe, expect, it } from 'vitest'
import { getSettings, setSettings } from './api'
import type { Settings } from './types'

const calls: Array<{ cmd: string; args: unknown }> = []

/** The desktop bridge, answering each command from its own queue, every call recorded. */
function backend(answers: Record<string, unknown[]>) {
  window.__TAURI_INTERNALS__ = {
    invoke: (async (cmd: string, args?: unknown) => {
      calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? null)) })
      return answers[cmd]?.shift() ?? {}
    }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'],
  }
}

afterEach(() => {
  delete window.__TAURI_INTERNALS__
  calls.length = 0
})

const saves = () => calls.filter((c) => c.cmd === 'set_settings').map((c) => c.args)

describe('a settings read and the save made from it', () => {
  it('takes the revision off a read, and a copy spread from it saves with that revision', async () => {
    backend({
      get_settings: [
        { mycall: 'KD9TAW', nexusRevision: 'rFirst' },
        { mycall: 'KD9TAW', nexusRevision: 'rLater' },
      ],
    })
    const first = await getSettings()
    expect(Object.keys(first)).toEqual(['mycall'])
    expect(JSON.parse(JSON.stringify(first))).toEqual({ mycall: 'KD9TAW' })
    await getSettings()
    await setSettings({ ...first, mycall: 'W1AW' })
    expect(saves()).toEqual([{ settings: { mycall: 'W1AW' }, base: 'rFirst' }])
  })

  it('control: a copy no read made, and a read at the station’s own screen, name none', async () => {
    backend({ get_settings: [{ mycall: 'KD9TAW' }] })
    await setSettings({ mycall: 'W1AW' } as Settings)
    const read = await getSettings()
    expect(read).toEqual({ mycall: 'KD9TAW' })
    await setSettings({ ...read })
    expect(saves()).toEqual([{ settings: { mycall: 'W1AW' } }, { settings: { mycall: 'KD9TAW' } }])
  })
})
