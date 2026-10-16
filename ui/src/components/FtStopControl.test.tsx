// @vitest-environment jsdom
//
// THE FT STRIP'S STOP TX ON THE DESKTOP. Its enabled state follows the Stop authority alone
// (useStationStopControl), and the desktop holds that authority: the button is live whatever the
// readings say, a press halts, and nothing beside it claims a Stop is still on its way.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { FtStopControl } from './FtStopControl'
import type { RadioStatus } from '../types'

afterEach(cleanup)

it('is live on the desktop, and a press halts', () => {
  const onHaltTx = vi.fn()
  render(<FtStopControl onHaltTx={onHaltTx} />)
  const stop = screen.getByRole('button', { name: 'Stop TX' }) as HTMLButtonElement
  expect(stop.disabled).toBe(false)
  fireEvent.click(stop)
  expect(onHaltTx).toHaveBeenCalledTimes(1)
})

it('says nothing about a Stop in flight: on the desktop the call is the act', () => {
  render(<FtStopControl onHaltTx={vi.fn()} radio={{ transmitting: true, txEnabled: true } as RadioStatus} />)
  expect(screen.queryByRole('status')).toBeNull()
})
