// @vitest-environment jsdom
// The banner of a computer driving another station. On a station's own screen it must say
// nothing and ask once; on a driving computer it must say which station and how the link stands,
// in every state the backend names, with why a refusal happened, and Leave must start this
// computer's Nexus again as itself.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import type { ClientStatus } from '../api'
import { EN } from '../i18n/en'

let desktop = true
const mockStatus = vi.fn<() => Promise<ClientStatus | null>>()
const mockLeave = vi.fn<() => Promise<void>>()
vi.mock('../api', () => ({
  isTauri: () => desktop,
  clientStatus: () => mockStatus(),
  leaveClientMode: () => mockLeave(),
}))
import { ClientBanner, CLIENT_STATUS_EVERY_MS } from './ClientBanner'

const AT = '192.168.1.20:42075'
const fill = (text: string, values: Record<string, string>) =>
  text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => values[name] ?? '')

beforeEach(() => {
  desktop = true
  vi.useFakeTimers()
  mockStatus.mockReset()
  mockLeave.mockReset().mockResolvedValue(undefined)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** Renders the banner and lets its first question be answered. */
async function shown(): Promise<HTMLElement> {
  const { container } = render(<ClientBanner />)
  await act(async () => {
    await Promise.resolve()
  })
  return container
}

describe('on a station’s own screen', () => {
  it('shows nothing, and asks once', async () => {
    mockStatus.mockResolvedValue(null)
    const container = await shown()
    expect(container.querySelector('.client-banner')).toBeNull()
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS * 10)
    })
    expect(mockStatus).toHaveBeenCalledTimes(1)
  })

  it('outside the desktop app it asks nothing at all', async () => {
    desktop = false
    const container = await shown()
    expect(container.querySelector('.client-banner')).toBeNull()
    expect(mockStatus).not.toHaveBeenCalled()
  })
})

describe('on a computer driving a station', () => {
  it('says which station and how the link stands, and follows it as it changes', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await shown()
    expect(screen.getByRole('status').textContent).toContain(
      fill(EN['client.banner.live'], { station: AT }),
    )
    mockStatus.mockResolvedValue({ station: AT, phase: 'notLive', reason: null })
    await act(async () => {
      vi.advanceTimersByTime(CLIENT_STATUS_EVERY_MS)
      await Promise.resolve()
    })
    const banner = screen.getByRole('status')
    expect(banner.textContent).toContain(fill(EN['client.banner.notLive'], { station: AT }))
    expect(banner.getAttribute('data-phase')).toBe('notLive')
  })

  it('names each state: connecting, another device in control', async () => {
    for (const [phase, key] of [
      ['connecting', 'client.banner.connecting'],
      ['busy', 'client.banner.busy'],
    ] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase, reason: null })
      await shown()
      expect(screen.getByRole('status').textContent).toContain(fill(EN[key], { station: AT }))
      cleanup()
    }
  })

  it('a refusal says why, in the words of Stations on this network where they have them', async () => {
    for (const [reason, sentence] of [
      ['updateStation', EN['lanWindow.reason.updateStation']],
      ['keyChanged', EN['lanWindow.reason.keyChanged']],
      ['otherBuild', EN['client.reason.otherBuild']],
      ['localPermissionRequired', EN['client.reason.localPermissionRequired']],
      ['somethingNew', EN['client.reason.unknown']],
    ] as const) {
      mockStatus.mockResolvedValue({ station: AT, phase: 'refused', reason })
      await shown()
      expect(screen.getByRole('status').textContent).toContain(
        fill(EN['client.banner.refused'], { station: AT, reason: sentence }),
      )
      cleanup()
    }
  })

  it('Leave starts this computer’s Nexus again as itself, once', async () => {
    mockStatus.mockResolvedValue({ station: AT, phase: 'live', reason: null })
    await shown()
    const leave = screen.getByRole('button', { name: EN['client.banner.leave'] })
    fireEvent.click(leave)
    expect(mockLeave).toHaveBeenCalledTimes(1)
    expect((leave as HTMLButtonElement).disabled).toBe(true)
  })
})
