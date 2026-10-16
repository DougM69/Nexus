// How a 3-D globe goes on screen: the Logbook's (QsoGlobe) and Connect's (Globe3D) each mount inside one.
//
// three.js draws them with WebGL 2 only, and THROWS when it cannot make that context ("THREE.WebGLRenderer:
// Error creating WebGL context."). Nothing between a globe and App's section boundary caught that, so on a
// computer without WebGL 2 (a report from Linux Mint on 1.18.0) the whole Logbook was the error page and the
// log could not be opened. So:
//   - where this computer cannot make the context three.js asks for (gpu.ts `webgl2ForGlobe`, asked once a
//     page), the host's note stands where the globe would be, and the globe's lazy three.js chunk never loads;
//   - where it can, the globe mounts inside a boundary of its own, so anything it throws later (a context that
//     never comes back and cannot be made again on Reload, a driver that fails) leaves the host's other note in
//     the globe's place. The view around it carries on.
// The context hand-back and the pause on a lost context are globeWebgl.tsx's; this is only the way in.
//
// ⚠️ NO three.js IMPORT HERE: the hosts import this statically and their globes lazily, so whatever this
// file imports is in the main bundle.
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The notes are props: each host owns its words.
import { Component, type ErrorInfo, type ReactNode } from 'react'
import { webgl2ForGlobe } from '../gpu'

interface Props {
  /** Where this computer cannot make the WebGL 2 context the globe needs. */
  noWebgl2: string
  /** Where the globe failed once it was on screen. */
  failed: string
  children: ReactNode
}

export function GlobeGuard({ noWebgl2, failed, children }: Props) {
  if (!webgl2ForGlobe()) return <GlobeNote text={noWebgl2} />
  return <GlobeBoundary failed={failed}>{children}</GlobeBoundary>
}

function GlobeNote({ text }: { text: string }) {
  return (
    <div className="globe-note" role="status">
      {text}
    </div>
  )
}

class GlobeBoundary extends Component<{ failed: string; children: ReactNode }, { error: boolean }> {
  state = { error: false }

  static getDerivedStateFromError(): { error: boolean } {
    return { error: true }
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    // Loud, as App's boundary is: the view shows a note, and the log says what failed.
    console.error('[nexus] 3-D globe failed:', error, info.componentStack)
  }

  render() {
    return this.state.error ? <GlobeNote text={this.props.failed} /> : this.props.children
  }
}
