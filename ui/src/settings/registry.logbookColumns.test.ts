// The Logbook's column choice is kept in settings.json, and its way back to the defaults is a
// Reset in Settings ▸ Appearance ▸ Map & globe, beside the Logbook's globe. A setting nobody can
// find is a setting nobody can undo, so the words an operator types to look for it must reach that
// section.
//
// POSITIVE CONTROL: a word the section already had still hits, so a green run cannot mean the
// search itself is broken.
import { describe, it, expect } from 'vitest'
import { searchSettings } from './registry'

describe('the Logbook columns can be found in Settings', () => {
  it.each(['logbook columns', 'columns', 'column width', 'more columns'])('searching "%s" reaches the section', (q) => {
    expect(searchSettings(q).map((h) => h.section.id)).toContain('map-globe')
  })

  it('positive control — a keyword that was already registered still hits', () => {
    expect(searchSettings('logbook globe').map((h) => h.section.id)).toContain('map-globe')
  })
})
