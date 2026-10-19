// @vitest-environment jsdom
// Frequency markers on the band map (features/freqMarkers.ts): each marker is drawn at its own
// frequency with its call; a click or Enter on it asks to tune back, Delete asks to remove it, a
// right-click on the track asks to mark the dial, and a marker never tunes the map's own way (the
// track's click-to-tune) as well. The station's half (refused while transmitting, cleared once
// logged, kept per contest session) is freq_markers.rs's.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { BandMap } from './BandMap'
import type { FreqMarker } from '../api'
import { isMarkKey } from '../features/freqMarkers'

vi.mock('../useScopeTune', () => ({ useScopeTune: () => scopeTune }))
const scopeTune = vi.fn()

afterEach(() => {
  cleanup()
  scopeTune.mockReset()
})

const w1aw: FreqMarker = { id: 7, dialHz: 14_031_200, band: '20M', call: 'W1AW' }
const bare: FreqMarker = { id: 8, dialHz: 14_012_000, band: '20M', call: '' }
const other: FreqMarker = { id: 9, dialHz: 7_025_000, band: '40M', call: 'K9XX' }

function draw(over: Partial<Parameters<typeof BandMap>[0]> = {}) {
  const props = {
    onMarkerQsy: vi.fn(),
    onMarkerDelete: vi.fn(),
    onMarkHere: vi.fn(),
  }
  render(
    <BandMap
      band="20m"
      dialMhz={14.025}
      txAllowed
      spots={[]}
      spotMode="CW"
      onWorkSpot={() => {}}
      markers={[w1aw, bare, other]}
      tuneEnabled
      {...props}
      {...over}
    />,
  )
  return props
}

describe('BandMap — frequency markers', () => {
  it("draws this band's markers with their call and frequency, and no other band's", () => {
    draw()
    const marks = screen.getAllByRole('button', { name: /^Marked / })
    expect(marks.map((m) => m.getAttribute('aria-label'))).toEqual([
      'Marked 14.0312 W1AW. Click or Enter: tune back here. Delete: remove the marker.',
      'Marked 14.0120. Click or Enter: tune back here. Delete: remove the marker.',
    ])
    expect(marks[0].textContent).toContain('W1AW')
    expect(screen.queryByText('K9XX')).toBeNull()
  })

  it('a click or Enter on a marker asks to tune back to THAT marker, and the track does not tune too', () => {
    // jsdom lays nothing out: give the track a height, or its click-to-tune never runs at all.
    const rect = vi
      .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockReturnValue({ top: 0, left: 0, bottom: 400, right: 200, width: 200, height: 400, x: 0, y: 0, toJSON: () => ({}) })
    const p = draw()
    // The control: a click on the track itself does tune.
    fireEvent.click(document.querySelector('.bandmap-track') as HTMLElement, { clientY: 100 })
    expect(scopeTune).toHaveBeenCalledTimes(1)
    scopeTune.mockReset()
    const mark = screen.getAllByRole('button', { name: /^Marked 14.0312/ })[0]
    fireEvent.click(mark)
    fireEvent.keyDown(mark, { key: 'Enter' })
    expect(p.onMarkerQsy.mock.calls).toEqual([[w1aw], [w1aw]])
    expect(p.onMarkerDelete).not.toHaveBeenCalled()
    expect(scopeTune).not.toHaveBeenCalled()
    rect.mockRestore()
  })

  it('Delete on a marker asks to remove it', () => {
    const p = draw()
    fireEvent.keyDown(screen.getAllByRole('button', { name: /^Marked 14.0120/ })[0], { key: 'Delete' })
    expect(p.onMarkerDelete.mock.calls).toEqual([[bare]])
    expect(p.onMarkerQsy).not.toHaveBeenCalled()
  })

  it('a right-click on the track asks to mark the dial, and the browser menu stays shut', () => {
    const p = draw()
    const track = document.querySelector('.bandmap-track') as HTMLElement
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    track.dispatchEvent(ev)
    expect(p.onMarkHere).toHaveBeenCalledTimes(1)
    expect(ev.defaultPrevented).toBe(true)
  })

  it('with no marker handlers the map is as it was: no right-click mark', () => {
    draw({ onMarkHere: undefined })
    const track = document.querySelector('.bandmap-track') as HTMLElement
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    track.dispatchEvent(ev)
    expect(ev.defaultPrevented).toBe(false)
  })
})

describe('Alt+M', () => {
  const k = (over: Partial<KeyboardEvent>) =>
    ({ key: 'm', code: 'KeyM', altKey: true, ctrlKey: false, metaKey: false, shiftKey: false, ...over }) as KeyboardEvent
  it('is Alt+M alone', () => {
    expect(isMarkKey(k({}))).toBe(true)
    expect(isMarkKey(k({ key: 'M' }))).toBe(true)
    // A Mac's Option+M types µ: the key's place decides.
    expect(isMarkKey(k({ key: 'µ' }))).toBe(true)
    expect(isMarkKey(k({ altKey: false }))).toBe(false)
    expect(isMarkKey(k({ ctrlKey: true }))).toBe(false)
    expect(isMarkKey(k({ shiftKey: true }))).toBe(false)
    expect(isMarkKey(k({ metaKey: true }))).toBe(false)
    expect(isMarkKey(k({ key: 'n', code: 'KeyN' }))).toBe(false)
  })
})
