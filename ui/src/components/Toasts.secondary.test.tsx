// @vitest-environment jsdom
//
// A toast's SECOND button (the radio switch notice: "Undo" and "Why?"). It is a plain link-like
// action: it runs and closes its toast. A toast with only the first button is drawn as before.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { Toasts } from './Toasts'
import { pushToast, dismissToast } from '../toast'

const ids: number[] = []
afterEach(() => {
  for (const id of ids.splice(0)) dismissToast(id)
  cleanup()
})

describe('a toast with a second action', () => {
  it('shows both buttons, and the second runs its own action and closes the toast', async () => {
    const first = vi.fn()
    const second = vi.fn()
    render(<Toasts />)
    ids.push(
      pushToast('two buttons', 'info', 0, {
        action: first,
        actionLabel: 'Undo',
        secondaryAction: second,
        secondaryLabel: 'Why?',
      }),
    )
    expect(await screen.findByRole('button', { name: /Undo/ })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Why\?/ }))
    expect(second).toHaveBeenCalledTimes(1)
    expect(first).not.toHaveBeenCalled()
    await waitFor(() => expect(screen.queryByText('two buttons')).toBeNull())
  })

  it('a toast with no second action draws no second button', async () => {
    render(<Toasts />)
    ids.push(pushToast('one button', 'info', 0, { action: () => {}, actionLabel: 'Go' }))
    await screen.findByText('one button')
    expect(document.querySelectorAll('.ui-toast-secondary')).toHaveLength(0)
  })
})
