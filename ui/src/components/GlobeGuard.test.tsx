// @vitest-environment jsdom
//
// GlobeGuard: how a 3-D globe goes on screen (the Logbook's and Connect's). Where this computer cannot make
// the WebGL 2 context three.js needs, the host's note stands where the globe would, and the globe (and its
// three.js chunk) never loads. Where it can, the globe mounts inside a boundary of its own: anything it throws,
// as it mounts or later, leaves the host's other note in its place, says what failed in the log, and costs
// nothing beside it.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { lazy, Suspense, useLayoutEffect, useState, type ReactNode } from 'react'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'

const gpu = vi.hoisted(() => ({ webgl2: true }))
vi.mock('../gpu', () => ({ webgl2ForGlobe: () => gpu.webgl2 }))
import { GlobeGuard } from './GlobeGuard'

const NO_WEBGL2 = 'This computer cannot draw it.'
const FAILED = 'It stopped.'

/** A stand-in globe: it mounts in a layout effect, as react-kapsule mounts globe.gl, and throws there (as
 *  three.js does when it cannot make its context) or on a later render, when told to. */
function Globe({ failOnMount = false, failNow = false }: { failOnMount?: boolean; failNow?: boolean }) {
  useLayoutEffect(() => {
    if (failOnMount) throw new Error('THREE.WebGLRenderer: Error creating WebGL context.')
  }, [failOnMount])
  if (failNow) throw new Error('the driver went away')
  return <canvas data-testid="globe" />
}

const guard = (child: ReactNode) => (
  <GlobeGuard noWebgl2={NO_WEBGL2} failed={FAILED}>
    {child}
  </GlobeGuard>
)
const note = () => screen.queryByRole('status')?.textContent ?? null

let errors: ReturnType<typeof vi.spyOn>
beforeEach(() => {
  gpu.webgl2 = true
  errors = vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('GlobeGuard', () => {
  it('where this computer makes no WebGL 2 context: the note, and the globe never loads', async () => {
    gpu.webgl2 = false
    const chunk = { loaded: false }
    const Lazy = lazy(async () => {
      chunk.loaded = true
      return { default: Globe }
    })
    render(guard(<Suspense fallback={null}><Lazy /></Suspense>))
    await act(async () => {})
    expect(note()).toBe(NO_WEBGL2)
    expect(screen.queryByTestId('globe')).toBeNull()
    expect(chunk.loaded, "the globe's chunk was fetched").toBe(false)
  })

  it('where it can: the globe, and no note', async () => {
    render(guard(<Globe />))
    expect(screen.getByTestId('globe')).toBeTruthy()
    expect(note()).toBeNull()
  })

  it('a globe that throws as it mounts: the note says it stopped, and the log says what failed', () => {
    render(guard(<Globe failOnMount />))
    expect(note()).toBe(FAILED)
    expect(screen.queryByTestId('globe')).toBeNull()
    const said = errors.mock.calls.map((c: unknown[]) => c.map(String).join(' ')).filter((l: string) => l.startsWith('[nexus] 3-D globe failed'))
    expect(said.join('\n')).toContain('Error creating WebGL context')
  })

  it('a globe that throws later: only the globe goes, and what is beside it carries on', () => {
    function Host() {
      const [fail, setFail] = useState(false)
      const [n, setN] = useState(0)
      return (
        <>
          {guard(<Globe failNow={fail} />)}
          <button type="button" onClick={() => setFail(true)}>
            fail
          </button>
          <button type="button" onClick={() => setN((x) => x + 1)}>
            {`count ${n}`}
          </button>
        </>
      )
    }
    render(<Host />)
    expect(screen.getByTestId('globe')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'fail' }))
    expect(note()).toBe(FAILED)
    expect(screen.queryByTestId('globe')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'count 0' }))
    expect(screen.getByRole('button', { name: 'count 1' })).toBeTruthy()
  })
})
