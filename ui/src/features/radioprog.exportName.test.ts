import { expect, it } from 'vitest'
import { programExportName } from './radioprog'

it('names a CHIRP and a channel-list export by the UTC day', () => {
  const when = new Date('2026-09-15T23:30:00-05:00')
  expect(programExportName('chirp', when)).toBe('nexus-chirp-2026-09-16.csv')
  expect(programExportName('csv', when)).toBe('nexus-channels-2026-09-16.csv')
})
