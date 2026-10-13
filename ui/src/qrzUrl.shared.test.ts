// @vitest-environment jsdom
//
// ONE QRZ PAGE PER STATION, FROM EVERY BUTTON. The report: an APRS iGate's card, PI1UTR-10, and
// its QRZ button opened https://www.qrz.com/db/PI1UTR10, which is somebody else's callsign. Three
// places build a QRZ link and each had its own copy of the rule: the desktop's `qrz_url`
// (src-tauri), the hosted Remote page's QRZ button and the DXpedition link. The copies also
// disagreed with each other on ties and on text outside ASCII.
//
// `__fixtures__/qrz-urls.json` is ONE table of what a callsign opens. This file holds the two
// browser builders to it, row by row, and `qrz_url_is_the_shared_tables` in src-tauri holds the
// desktop's to the same rows. Change a row and all three must agree again.
import { describe, expect, it, vi, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dxpedLink } from './components/prop/dxpedLink'
import { RemoteCollections } from './remote-web/collections'
import type { ApplicationClient } from './remote-web/application-client'

interface Case {
  call: string
  url: string | null
  why: string
}
// A path, not `new URL(…, import.meta.url)`: under jsdom that URL is not a file URL Node can read.
const TABLE = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '__fixtures__/qrz-urls.json'), 'utf8')) as {
  cases: Case[]
}

afterEach(() => vi.restoreAllMocks())

describe('the QRZ page a callsign opens', () => {
  it('the table only ever blesses a QRZ page of letters and digits', () => {
    expect(TABLE.cases.length).toBeGreaterThan(0)
    for (const { call, url } of TABLE.cases) {
      if (url !== null) expect(url, call).toMatch(/^https:\/\/www\.qrz\.com\/db\/[A-Z0-9]+$/)
    }
  })

  it.each(TABLE.cases)('the DXpedition link for "$call" ($why)', ({ call, url }) => {
    expect(dxpedLink({ call })).toEqual(url === null ? null : { kind: 'qrz', url, label: 'QRZ' })
  })

  it.each(TABLE.cases)('the Remote page QRZ button for "$call" ($why)', async ({ call, url }) => {
    const invoke = vi.fn(), open = vi.spyOn(window, 'open').mockReturnValue(null)
    const source = new RemoteCollections({ invoke, getPhase: () => 'ready' } as unknown as ApplicationClient)
    const opened = source.invoke('open_qrz_page', { call })
    if (url === null) {
      await expect(opened).rejects.toThrow('applicationUnsupported')
      expect(open).not.toHaveBeenCalled()
    } else {
      await opened
      expect(open).toHaveBeenCalledExactlyOnceWith(url, '_blank', 'noopener,noreferrer')
    }
    // Handled in the browser: it never reaches the station.
    expect(invoke).not.toHaveBeenCalled()
  })
})
