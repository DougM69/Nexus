// @vitest-environment jsdom
//
// The seat swap is a MID-QSO act — that is what the operator field is for, handing the key
// over while the station keeps running. Writing it through the whole-settings save ran the
// engine's heavyweight apply (#54): mode back to Chat, TX queue cleared, TX cycle re-derived
// from whatever settings snapshot this component was holding. #100 has since narrowed the
// mode reset to Field Day transitions only, but the other two remain — the write has to stay
// narrow.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { ContestView } from './ContestView'
import { setFdOperator, setSettings } from '../api'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

vi.mock('../api', () => ({
  // The contest screen's Removed list, read on mount: none removed.
  contestRemoved: vi.fn(async () => []),
  contestUnmerged: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ ...defaultSettings, fdOperator: '' })),
  setSettings: vi.fn(async () => ({})),
  setFdOperator: vi.fn(async () => ({})),
  exportLog: vi.fn(async () => ''),
  openPanelWindow: vi.fn(async () => {}),
}))

const FD: FieldDayStatus = {
  composing: [
    { key: 'CLASS', raw: '1A' },
    { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
  ],
  running: true,
  state: 'sp',
  qsoCount: 0,
  sections: 0,
  points: 0,
  log: [],
}

const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
}

beforeEach(() => {
  vi.mocked(setFdOperator).mockClear()
  vi.mocked(setSettings).mockClear()
})
afterEach(() => cleanup())

describe('Field Day operator swap', () => {
  it('persists the new operator through the narrow write, not the whole-settings save', async () => {
    render(<ContestView fieldDay={FD} onSetMode={() => {}} />)
    await settle()

    const input = screen.getByLabelText('Operator (call or initials)')
    fireEvent.change(input, { target: { value: 'w1abc' } })
    fireEvent.blur(input)
    await settle()

    expect(vi.mocked(setFdOperator).mock.calls).toEqual([['W1ABC']])
    expect(vi.mocked(setSettings)).not.toHaveBeenCalled()
  })
})

// ⭐ CTRL+O FROM A CONTEST STRIP (`features/contestKeys.ts`): the window shows this screen with an ask
// pending, and the operator box takes the caret as soon as it can be typed in (its settings read),
// once per ask, and says so.
describe('the operator box answers Ctrl+O', () => {
  const box = () => screen.getByLabelText('Operator (call or initials)') as HTMLInputElement

  it('takes the caret once its settings are read, and says it did', async () => {
    const focused = vi.fn()
    render(<ContestView fieldDay={FD} onSetMode={() => {}} operatorAsk={1} onOperatorFocused={focused} />)
    expect(document.activeElement, 'not before the box can be typed in').not.toBe(box())
    await settle()
    expect(document.activeElement).toBe(box())
    expect(focused).toHaveBeenCalledTimes(1)
  })

  it('takes it again at the next ask, and never without one', async () => {
    const focused = vi.fn()
    const view = render(<ContestView fieldDay={FD} onSetMode={() => {}} onOperatorFocused={focused} />)
    await settle()
    expect(document.activeElement).not.toBe(box())
    view.rerender(<ContestView fieldDay={FD} onSetMode={() => {}} operatorAsk={1} onOperatorFocused={focused} />)
    await settle()
    expect(document.activeElement).toBe(box())
    box().blur()
    view.rerender(<ContestView fieldDay={FD} onSetMode={() => {}} operatorAsk={2} onOperatorFocused={focused} />)
    await settle()
    expect(document.activeElement).toBe(box())
    expect(focused).toHaveBeenCalledTimes(2)
  })

  it('a Remote observer’s read-only box never takes it', async () => {
    const focused = vi.fn()
    render(
      <ContestView
        fieldDay={FD}
        observation={{ fdOperator: 'K1ABC', fdPowerMult: 2, fdBonuses: [], fdBonusesPlanned: [] }}
        operatorAsk={1}
        onOperatorFocused={focused}
      />,
    )
    await settle()
    expect(document.activeElement).not.toBe(box())
    expect(focused).not.toHaveBeenCalled()
  })
})
