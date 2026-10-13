// @vitest-environment jsdom
//
// ⭐ EVERY LAPTOP'S MULTIPLIER BOARD LIGHTS WHAT THE CLUB HAS WORKED. A county one position works
// lights on the host's board and on every other laptop's (`ContestBoard.club`, the host's club log
// or what the host sent), and this laptop's own contacts (`worked`) are marked apart from the rest:
// a mark AND a colour, never the colour alone, with a legend that says which is which.
//
// A board with no club list (no club sync, a host older than the list, Remote) is this laptop's own
// board exactly as it always was: same marks, same words, no legend.
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { MultiplierBoards } from './ContestView'
import type { ContestBoard } from '../types'

afterEach(cleanup)

const COUNTY = (over: Partial<ContestBoard> = {}): ContestBoard => ({
  id: 'county',
  slot: 'QTH',
  domain: 'il_counties',
  scope: 'perLog',
  worked: [],
  ...over,
})

/** The cell for `code` on the board labelled `board`: the span that carries its words. */
function cell(board: HTMLElement, code: string): HTMLElement {
  const found = [...board.querySelectorAll<HTMLElement>('span[aria-label]')].find((s) =>
    (s.textContent ?? '').endsWith(code),
  )
  if (!found) throw new Error(`no cell ${code}`)
  return found
}

describe('the club-wide multiplier board', () => {
  it('lights a county another position worked, and marks this laptop’s own apart', () => {
    render(<MultiplierBoards boards={[COUNTY({ worked: ['COOK'], club: ['COOK', 'WILL'] })]} />)
    const board = screen.getByLabelText('county multiplier board')
    // This laptop's own: its own mark and words.
    expect(cell(board, 'COOK').textContent).toBe('⌂COOK')
    expect(cell(board, 'COOK').getAttribute('aria-label')).toBe('Cook, worked on this laptop')
    expect(cell(board, 'COOK').dataset.worked).toBe('own')
    // Another position's: lit, with the worked mark and its own words.
    expect(cell(board, 'WILL').textContent).toBe('✓WILL')
    expect(cell(board, 'WILL').getAttribute('aria-label')).toBe('Will, worked by another position')
    expect(cell(board, 'WILL').dataset.worked).toBe('club')
    // What the club still needs reads as it always did.
    expect(cell(board, 'ADAM').textContent).toBe('ADAM')
    expect(cell(board, 'ADAM').getAttribute('aria-label')).toBe('Adams, not worked')
    // The count is the club's, with this laptop's beside it.
    expect(board.textContent).toContain('2/102 worked · 1 on this laptop')
    // The legend says which mark is which, each in its cell's own look.
    const legend = board.querySelector<HTMLElement>('[data-board-legend]')
    expect(legend).not.toBeNull()
    expect(legend!.textContent).toContain('⌂ Worked on this laptop')
    expect(legend!.textContent).toContain('✓ Worked by another position')
  })

  it('marks a county this laptop and another position both worked as this laptop’s', () => {
    render(<MultiplierBoards boards={[COUNTY({ worked: ['COOK', 'WILL'], club: ['COOK', 'WILL'] })]} />)
    const board = screen.getByLabelText('county multiplier board')
    expect(cell(board, 'WILL').dataset.worked).toBe('own')
    expect(board.textContent).toContain('2/102 worked · 2 on this laptop')
  })

  it('shows this laptop’s own contact before the host has it back', () => {
    // A contact logged here that has not reached the host's list yet is still this laptop's own.
    render(<MultiplierBoards boards={[COUNTY({ worked: ['COOK'], club: ['WILL'] })]} />)
    const board = screen.getByLabelText('county multiplier board')
    expect(cell(board, 'COOK').dataset.worked).toBe('own')
    expect(cell(board, 'WILL').dataset.worked).toBe('club')
    expect(board.textContent).toContain('2/102 worked · 1 on this laptop')
  })

  it('a Field Day club lights the sections board the same way', () => {
    render(
      <MultiplierBoards
        boards={[
          {
            id: 'SECTION',
            slot: 'SECTION',
            domain: 'fd_sections',
            scope: 'perLog',
            worked: ['IL'],
            club: ['EMA', 'IL'],
          },
        ]}
      />,
    )
    const board = screen.getByLabelText('Worked sections board')
    expect(cell(board, 'IL').getAttribute('aria-label')).toBe('Illinois, worked on this laptop')
    expect(cell(board, 'EMA').getAttribute('aria-label')).toBe(
      'Eastern Massachusetts, worked by another position',
    )
    expect(cell(board, 'EMA').title).toBe(
      'EMA — Eastern Massachusetts (New England) — worked by another position',
    )
    expect(board.textContent).toContain('2/85 sections · 1 on this laptop')
    expect(board.querySelector('[data-board-legend]')).not.toBeNull()
  })

  it('lights one province under either of its spellings, and counts it once', () => {
    // The Illinois QSO Party's list carries Canada Post's PE and NL beside the sponsor's PEI and
    // NF: this laptop worked a PE, another position an NF.
    render(
      <MultiplierBoards
        boards={[
          { id: 'mult', slot: 'QTH', domain: 'il_mults', scope: 'perLog', worked: ['PE'], club: ['PE', 'NF'] },
        ]}
      />,
    )
    const board = screen.getByLabelText('mult multiplier board')
    expect(['PE', 'PEI', 'NL', 'NF'].map((c) => cell(board, c).dataset.worked)).toEqual([
      'own',
      'own',
      'club',
      'club',
    ])
    expect(board.textContent).toContain('2/64 worked · 1 on this laptop')
    // CONTROL: another province is its own cell.
    expect(cell(board, 'NS').dataset.worked).toBe('none')
  })

  it('CONTROL: a board with no club list is this laptop’s own, exactly as before', () => {
    render(<MultiplierBoards boards={[COUNTY({ worked: ['COOK'] })]} />)
    const board = screen.getByLabelText('county multiplier board')
    expect(cell(board, 'COOK').textContent).toBe('✓COOK')
    expect(cell(board, 'COOK').getAttribute('aria-label')).toBe('Cook, worked')
    expect(cell(board, 'WILL').getAttribute('aria-label')).toBe('Will, not worked')
    expect(board.textContent).toContain('1/102 worked')
    expect(board.textContent).not.toContain('on this laptop')
    expect(board.querySelector('[data-board-legend]')).toBeNull()
  })

  it('CONTROL: an empty club list is no club list', () => {
    render(<MultiplierBoards boards={[COUNTY({ worked: ['COOK'], club: [] })]} />)
    const board = screen.getByLabelText('county multiplier board')
    expect(cell(board, 'COOK').getAttribute('aria-label')).toBe('Cook, worked')
    expect(board.querySelector('[data-board-legend]')).toBeNull()
  })
})
