// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useRadioSwitchNotice, type RadioSwitchNoticeDeps } from './useRadioSwitchNotice'
import { dismissToast, subscribeToasts, type Toast } from './toast'
import type { AutoRadioSwitch, RadioSwitchPolicy, RadioSummary } from './types'

const radios = [
  { id: 0, name: 'FTdx10' },
  { id: 1, name: 'IC-9700' },
] as RadioSummary[]

let toasts: Toast[] = []
const unsub = subscribeToasts((ts) => (toasts = ts))
afterEach(() => {
  for (const x of toasts) dismissToast(x.id)
})
void unsub

const sw = (seq: number, fromId = 0, toId = 1, band = '2m'): AutoRadioSwitch => ({ seq, fromId, toId, band, mode: 'digital' })

function mount(policy: RadioSwitchPolicy | undefined, first?: AutoRadioSwitch) {
  const selectRadio = vi.fn(() => Promise.resolve())
  const openSettingsAt = vi.fn()
  const hook = renderHook((p: RadioSwitchNoticeDeps) => useRadioSwitchNotice(p), {
    initialProps: { snap: { radios, autoRadioSwitch: first }, policy, selectRadio, openSettingsAt },
  })
  const next = (s: AutoRadioSwitch | undefined, pol = policy) =>
    hook.rerender({ snap: { radios, autoRadioSwitch: s }, policy: pol, selectRadio, openSettingsAt })
  return { next, selectRadio, openSettingsAt }
}

describe('the radio switch notice', () => {
  it('says a switch Nexus made by itself, with Undo back to the radio it left and Why? to the rules', async () => {
    const { next, selectRadio, openSettingsAt } = mount('tell')
    expect(toasts).toHaveLength(0)
    next(sw(1))
    expect(toasts).toHaveLength(1)
    const [n] = toasts
    expect(n.message).toBe('Switched to IC-9700 for 2m · Weak-signal digital')
    expect(n.kind).toBe('info')
    expect(n.actionLabel).toBe('Undo')
    await n.action?.()
    expect(selectRadio).toHaveBeenCalledWith(0)
    expect(n.secondaryLabel).toBe('Why?')
    n.secondaryAction?.()
    expect(openSettingsAt).toHaveBeenCalledWith('radios')
  })

  it('a switch made before the window opened is not news', () => {
    const { next } = mount('tell', sw(4))
    expect(toasts).toHaveLength(0)
    next(sw(4))
    expect(toasts).toHaveLength(0)
    next(sw(5, 1, 0, '20m'))
    expect(toasts.map((x) => x.message)).toEqual(['Switched to FTdx10 for 20m · Weak-signal digital'])
  })

  it('a station that keeps the silent switch sees nothing, and nothing shows before the policy is read', () => {
    const silent = mount('act')
    silent.next(sw(1))
    expect(toasts).toHaveLength(0)
    const unread = mount(undefined)
    unread.next(sw(1))
    expect(toasts).toHaveLength(0)
  })

  it('a newer switch replaces the notice still up, one line at a time', () => {
    const { next } = mount('tell')
    next(sw(1))
    next(sw(2, 1, 0, '20m'))
    expect(toasts.map((x) => x.message)).toEqual(['Switched to FTdx10 for 20m · Weak-signal digital'])
  })
})
