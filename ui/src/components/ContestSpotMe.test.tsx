// @vitest-environment jsdom
//
// The contest screen's Spot me: ONE cluster spot per operator press, never on a timer, a cooldown
// after a spot went, and every refusal said in words. The backend holds the same cooldown
// (contest_spot.rs); this pins the button's half.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'

const contestSelfSpot = vi.fn()
vi.mock('../api', () => ({ contestSelfSpot: (c: string) => contestSelfSpot(c) }))
const pushToast = vi.fn()
vi.mock('../toast', () => ({ pushToast: (...a: unknown[]) => pushToast(...a) }))

import { ContestSpotMe, defaultSpotComment } from './ContestSpotMe'

const posted = { freqKhz: 14025.5, call: 'KD9TAW', comment: 'ILQP CQ', cooldownSecs: 60 }

beforeEach(() => {
  vi.useFakeTimers()
  contestSelfSpot.mockReset()
  pushToast.mockReset()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const button = () => screen.getByRole('button', { name: 'Spot me' })
const settle = async () => {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('Spot me', () => {
  it('defaults the comment to the contest short name and CQ, and the operator can edit it', () => {
    expect(defaultSpotComment('ILQP')).toBe('ILQP CQ')
    expect(defaultSpotComment('')).toBe('CQ')
    render(<ContestSpotMe contestShort="ILQP" />)
    const box = screen.getByRole('textbox', { name: 'Spot comment' }) as HTMLInputElement
    expect(box.value).toBe('ILQP CQ')
    fireEvent.change(box, { target: { value: 'ILQP CQ 599' } })
    expect(box.value).toBe('ILQP CQ 599')
  })

  it('posts nothing by itself: ten minutes on screen, no spot', async () => {
    render(<ContestSpotMe contestShort="ILQP" />)
    await act(async () => {
      vi.advanceTimersByTime(10 * 60_000)
    })
    expect(contestSelfSpot).not.toHaveBeenCalled()
  })

  it('one press posts one spot with the edited comment and says the line that went', async () => {
    contestSelfSpot.mockResolvedValue(posted)
    render(<ContestSpotMe contestShort="ILQP" />)
    fireEvent.change(screen.getByRole('textbox', { name: 'Spot comment' }), { target: { value: 'ILQP CQ' } })
    fireEvent.click(button())
    await settle()
    expect(contestSelfSpot.mock.calls).toEqual([['ILQP CQ']])
    expect(pushToast).toHaveBeenCalledWith('Spotted to the DX cluster: DX 14025.5 KD9TAW ILQP CQ', 'success')
  })

  it('a double-click is one spot, and the button waits out the cooldown before the next', async () => {
    let resolve: (v: typeof posted) => void = () => {}
    contestSelfSpot.mockImplementation(() => new Promise((r) => (resolve = r)))
    render(<ContestSpotMe contestShort="ILQP" />)
    fireEvent.click(button())
    fireEvent.click(button())
    expect(contestSelfSpot).toHaveBeenCalledTimes(1)
    resolve(posted)
    await settle()
    // Cooling: the button says how long, and a press posts nothing.
    expect((screen.getByRole('button', { name: 'Wait 60 s' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Wait 60 s' }))
    await act(async () => {
      vi.advanceTimersByTime(30_000)
    })
    expect((screen.getByRole('button', { name: 'Wait 30 s' }) as HTMLButtonElement).disabled).toBe(true)
    await act(async () => {
      vi.advanceTimersByTime(30_000)
    })
    expect((button() as HTMLButtonElement).disabled).toBe(false)
    expect(contestSelfSpot).toHaveBeenCalledTimes(1)
  })

  it('two presses before the screen redraws are still one spot', async () => {
    contestSelfSpot.mockResolvedValue(posted)
    render(<ContestSpotMe contestShort="ILQP" />)
    const b = button()
    await act(async () => {
      b.click()
      b.click()
    })
    expect(contestSelfSpot).toHaveBeenCalledTimes(1)
  })

  it.each([
    ['spotNoCall', 'Not spotted: set your callsign in Settings first.'],
    ['spotNoCluster', 'Not spotted: no DX cluster is connected. Set a cluster in Settings ▸ Spot Sources.'],
    ['spotCooldown:42', 'Not spotted again yet: one spot a minute. 42 s to go.'],
    ['spotRfOnly', 'Not spotted: this contest allows spotting only over amateur radio while it runs.'],
    ['spotNotAllowed', "Not spotted: this contest's rules allow no spotting."],
  ])('a refusal (%s) is said, and the button is free again at once', async (token, words) => {
    contestSelfSpot.mockRejectedValue(token)
    render(<ContestSpotMe contestShort="ILQP" />)
    fireEvent.click(button())
    await settle()
    expect(pushToast).toHaveBeenCalledWith(words, 'error')
    expect((button() as HTMLButtonElement).disabled).toBe(false)
  })
})
