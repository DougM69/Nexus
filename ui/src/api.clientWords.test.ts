// @vitest-environment jsdom
// A call that client mode refused, or could not make, fails with `nexusClient:<code>` from the
// backend (`src-tauri/src/client_mode.rs`). The screen must say it in the operator's language, at
// the one place every call passes, and must leave every other error exactly as it came.
import { afterEach, describe, expect, it } from 'vitest'
import { getSettings } from './api'
import { EN } from './i18n/en'

function backendRejects(error: unknown) {
  window.__TAURI_INTERNALS__ = {
    invoke: (async () => {
      throw error
    }) as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'],
  }
}

afterEach(() => {
  delete window.__TAURI_INTERNALS__
})

describe('a client-mode refusal, in words', () => {
  it('says each code the backend gives', async () => {
    for (const [code, key] of [
      ['changedAtShack', 'client.refused.changedAtShack'],
      ['transmitNotYet', 'client.refused.transmitNotYet'],
      ['heldPress', 'client.refused.heldPress'],
      ['noStopRoad', 'client.refused.noStopRoad'],
      ['noPresence', 'client.refused.noPresence'],
      ['staleState', 'client.refused.staleState'],
      ['pressEnded', 'client.refused.pressEnded'],
      ['recording', 'client.refused.recording'],
      ['shackOnly', 'client.refused.shackOnly'],
      ['notLive', 'client.refused.notLive'],
      ['gone', 'client.refused.gone'],
      ['unclassed', 'client.refused.unclassed'],
    ] as const) {
      backendRejects(`nexusClient:${code}`)
      await expect(getSettings()).rejects.toBe(EN[key])
    }
  })

  it('a code this build has no words for still says the station refused it', async () => {
    backendRejects('nexusClient:somethingNew')
    await expect(getSettings()).rejects.toBe(EN['client.refused.unknown'])
  })

  it('CONTROL: any other error comes through exactly as it came', async () => {
    backendRejects('No LoTW password stored — set it in Settings.')
    await expect(getSettings()).rejects.toBe('No LoTW password stored — set it in Settings.')
    backendRejects('nexusClient: not a code')
    await expect(getSettings()).rejects.toBe('nexusClient: not a code')
    const error = new Error('boom')
    backendRejects(error)
    await expect(getSettings()).rejects.toBe(error)
  })
})
