// @vitest-environment jsdom
// Pounce reaches the screen as an alert made once in Rust, with its own id (`src-tauri/src/alert.rs`):
// the screen listens for `alert`, takes kind `pounce` in the shape it reads, and an alert it has
// already shown, by its id, is not shown again; a computer driving the station hears the same alert
// under the same id. CONTROL: a new id is a new alert, even with the same call.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'

const { doubleBeep, osNotify } = vi.hoisted(() => ({
  doubleBeep: vi.fn(),
  osNotify: vi.fn(async () => {}),
}))
vi.mock('./alerts', () => ({ doubleBeep }))
vi.mock('./api', () => ({ osNotify }))
import { usePounce } from './usePounce'

type Handler = (event: { payload: unknown }) => void
const handlers = new Map<string, Handler>()

function bridge() {
  ;(window as unknown as { __TAURI__: unknown }).__TAURI__ = {
    event: {
      listen: async (name: string, handler: Handler) => {
        handlers.set(name, handler)
        return () => handlers.delete(name)
      },
    },
  }
}

afterEach(() => {
  delete (window as unknown as { __TAURI__?: unknown }).__TAURI__
  handlers.clear()
  doubleBeep.mockClear()
})

const pounce = { call: 'VP8PJ', band: '20m', mode: 'CW', freqMhz: 14.023, tags: [], entity: 'Falkland Is.', atUnix: 1 }
const raised = (id: number, over: Record<string, unknown> = {}) => ({ v: 1, id, kind: 'pounce', atMs: 5, alert: pounce, ...over })

async function hear(payload: unknown) {
  await act(async () => {
    handlers.get('alert')?.({ payload })
  })
}

describe('Pounce, heard as the alert the station made', () => {
  it('listens for the alert, shows Pounce’s once by its id, and leaves other kinds and shapes', async () => {
    bridge()
    const { result } = renderHook(() => usePounce())
    await act(async () => {})
    expect([...handlers.keys()]).toEqual(['alert'])
    await hear(raised(7))
    expect(result.current.alert).toEqual(pounce)
    expect(doubleBeep).toHaveBeenCalledTimes(1)
    await hear(raised(7))
    expect(doubleBeep, 'the same alert, twice').toHaveBeenCalledTimes(1)
    await hear(raised(8, { kind: 'newDxcc' }))
    await hear(raised(9, { v: 2 }))
    expect(doubleBeep, 'another kind, or a later shape').toHaveBeenCalledTimes(1)
    await hear(raised(10))
    expect(doubleBeep, 'CONTROL: a new id is a new alert').toHaveBeenCalledTimes(2)
  })
})
