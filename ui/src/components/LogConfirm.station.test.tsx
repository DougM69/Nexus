// @vitest-environment jsdom
//
// The prompt-to-log popup carries WSJT-X's Log QSO dialog's Prop Mode, Tx power and Operator
// boxes, and the grid the contact was made from. They open filled with what Nexus stamped on the
// contact, so a popup left alone logs exactly that; what the operator changes is logged.

import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { LogConfirm } from './LogConfirm'
import { t } from '../i18n'
import type { LoggedQso } from '../types'

const QSO = {
  call: 'DL1ABC',
  grid: 'JO62',
  band: '6m',
  mode: 'FT8',
  rstSent: '-12',
  rstRcvd: '-09',
  comment: null,
  txPower: 100,
  operator: 'K2DEF',
  myGrid: 'FN31',
  propMode: null,
} as unknown as LoggedQso

afterEach(cleanup)

function open(record: LoggedQso = QSO) {
  const onConfirm = vi.fn()
  render(<LogConfirm record={record} onConfirm={onConfirm} onDiscard={() => {}} />)
  const box = (label: Parameters<typeof t>[0]) => screen.getByLabelText(t(label)) as HTMLInputElement
  return {
    onConfirm,
    power: box('logPrompt.power.label'),
    operator: box('logPrompt.operator.label'),
    myGrid: box('logPrompt.myGrid.label'),
    propMode: screen.getByLabelText(t('logPrompt.propMode.label')) as HTMLSelectElement,
    log: () => fireEvent.click(screen.getByRole('button', { name: t('logPrompt.log') })),
  }
}

describe('the log prompt station boxes and prop mode', () => {
  it('opens filled with what the contact carries', () => {
    const p = open()
    expect(p.power.value).toBe('100')
    expect(p.operator.value).toBe('K2DEF')
    expect(p.myGrid.value).toBe('FN31')
    expect(p.propMode.value).toBe('')
  })

  it('sends what it opened with when left alone', () => {
    const p = open()
    p.log()
    const sent = p.onConfirm.mock.calls[0][0]
    expect([sent.txPower, sent.operator, sent.myGrid, sent.propMode]).toEqual([100, 'K2DEF', 'FN31', null])
  })

  it('logs what the operator set', () => {
    const p = open()
    fireEvent.change(p.power, { target: { value: ' 5 ' } })
    fireEvent.change(p.operator, { target: { value: 'n0op' } })
    fireEvent.change(p.myGrid, { target: { value: 'em48ab' } })
    fireEvent.change(p.propMode, { target: { value: 'ES' } })
    p.log()
    const sent = p.onConfirm.mock.calls[0][0]
    expect([sent.txPower, sent.operator, sent.myGrid, sent.propMode]).toEqual([5, 'N0OP', 'EM48AB', 'ES'])
  })

  it('a cleared box sends nothing, for Nexus to fill in', () => {
    const p = open()
    fireEvent.change(p.power, { target: { value: '' } })
    fireEvent.change(p.operator, { target: { value: ' ' } })
    p.log()
    const sent = p.onConfirm.mock.calls[0][0]
    expect([sent.txPower, sent.operator]).toEqual([null, null])
  })

  it("offers WSJT-X's propagation modes, and Satellite only on a satellite contact", () => {
    const p = open()
    const codes = Array.from(p.propMode.options).map((o) => o.value)
    for (const code of ['', 'EME', 'ES', 'MS', 'TR', 'F2', 'AUR', 'TEP']) expect(codes).toContain(code)
    expect(codes).not.toContain('SAT')
    cleanup()
    const sat = open({ ...QSO, propMode: 'SAT', satName: 'RS-44' } as unknown as LoggedQso)
    expect(sat.propMode.value).toBe('SAT')
  })
})
