// @vitest-environment jsdom
//
// #343: "where are the meters?" The TX meters (power, ALC, SWR, compression) are a panel, not a
// setting, so a search in Settings found nothing that answered. The box now says where they are.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SettingsSearch } from './SettingsSearch'

afterEach(cleanup)

const type = (q: string) => {
  render(<SettingsSearch onPick={vi.fn()} />)
  fireEvent.change(screen.getByRole('combobox'), { target: { value: q } })
}

describe('a search for the meters says they are a panel', () => {
  for (const q of ['meter', 'SWR', 'alc', 'TX meters', 'compression']) {
    it(`"${q}" points at ⊞ Panels ▸ TX Meters`, () => {
      type(q)
      expect(screen.getByText(/⊞ Panels ▸ TX Meters/)).toBeTruthy()
    })
  }

  it('a search for something else does not', () => {
    type('sound card')
    expect(screen.queryByText(/⊞ Panels/)).toBeNull()
  })
})
