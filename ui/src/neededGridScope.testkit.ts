// THE NEEDED BOARD'S BAND SCOPE, ONE FIXTURE FOR BOTH WINDOWS. Report on a release candidate: "I am now
// seeing hf grids in my needed when I dont have the filter chip selected". The board in its own window
// (it opens there at launch by default) listed every 20 m new grid while Settings ▸ Spots & Alerts held
// New grid at VHF+, which the docked board had kept since `boardNeeds` (features/needs.ts). The windows'
// tests read their boards against this one list, so the two windows can only agree.
//
// It imports nothing that reaches `./api`: App's test loads it inside its `./api` mock, and a module there
// that imports the module being mocked waits on itself forever (features/needs.ts does, through alerts.ts).
import type { NeedAlert, NeedTag, Settings } from './types'
import defaults from './components/__fixtures__/defaultSettings.json'

/** The backend's tiers for the lead tags used here (needalert.rs `NeedTag::tier`). */
const TIER: Partial<Record<NeedTag, number>> = { NewEntity: 100, NewGrid: 55, NewBand: 50 }

/** Saved alert settings as the report's: New grid VHF+ (6 m and up), the 💎 rare grid on all bands,
 *  new DXCC on all bands, and the alert master off, which no board reads. */
export const REPORT_SETTINGS = {
  ...(defaults as unknown as Settings),
  alertNew: false,
  alertGridBands: 'vhf',
  alertRareGridBands: 'all',
  alertDxccBands: 'all',
} as Settings

function need(call: string, band: string, freqMhz: number, mode: string, tags: NeedTag[], over: Partial<NeedAlert> = {}): NeedAlert {
  return {
    call, entity: 'United States', band, zone: 5, tags, priority: TIER[tags[0]] ?? 0, headline: `${tags[0]} ${call}`,
    mode, freqMhz, gridRarity: tags.includes('NewGrid') ? 'common' : null, park: null, ...over,
  }
}

/** What the engine hands the boards: every reason it scored, on every band. */
export const ROWS: NeedAlert[] = [
  need('VK9XQ', '20m', 14.074, 'Digital', ['NewEntity'], { entity: 'Christmas Island', zone: 29 }),
  // THE REPORT: a 20 m station whose only reason is its grid.
  need('K1GRD', '20m', 14.074, 'Digital', ['NewGrid']),
  // Two reasons on 20 m: the band slot stays when the grid goes.
  need('EA8TWO', '20m', 14.074, 'Digital', ['NewGrid', 'NewBand'], { entity: 'Canary Islands', zone: 33 }),
  // A rare square follows the 💎 scope, which this station keeps on all bands.
  need('VP2RAR', '20m', 14.075, 'Digital', ['NewGrid'], { gridRarity: 'rare', entity: 'Montserrat', zone: 8 }),
  need('K2SIX', '6m', 50.313, 'Digital', ['NewGrid']),
  need('W5BTH', '6m', 50.313, 'Digital', ['NewGrid', 'NewBand']),
  // A CW need while the CW cockpit is switched off: the board's own mode chips decide it, not the feature.
  need('K1CW', '20m', 14.025, 'CW', ['NewBand']),
  // A Phone park activation in a new 20 m grid: the park is the reason left.
  need('K9ABC', '20m', 14.285, 'Phone', ['NewGrid', 'NewPark', 'Pota'], {
    park: { program: 'POTA', reference: 'US-1000' },
  }),
]

/** The board for ROWS under REPORT_SETTINGS with All lit: each row's call and the reasons it shows. */
export const REPORT_BOARD = [
  'VK9XQ NewEntity',
  'EA8TWO NewBand',
  'VP2RAR NewGrid',
  'K2SIX NewGrid',
  'W5BTH NewGrid NewBand',
  'K1CW NewBand',
  'K9ABC NewPark Pota',
]

/** Read a board as the operator does — each row's call and the reasons it carries — out of its own
 *  row labels, which name the raw tags in every language. Sorted: the board's order is its sort's. */
export function boardRows(scope: ParentNode): string[] {
  return ([...scope.querySelectorAll('.np-row:not(.np-header)')] as HTMLElement[])
    .map((r) => {
      const label = r.getAttribute('aria-label') ?? ''
      return `${label.split(',')[0]} ${label.match(/needed (.*)$/)?.[1] ?? '?'}`
    })
    .sort()
}
