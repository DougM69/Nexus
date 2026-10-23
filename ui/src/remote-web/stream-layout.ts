// The stream page's layout for the window it has (the operator's pick, 2026-10-03: the phone layout). A phone held
// upright gets a bar above the picture for Stop TX and a bar under it for the thumbs; turned on its side it gets a
// rail of controls beside the picture, which then has the window's whole height; every other window keeps the header
// row the page has always had. It is decided from the effective size useViewport publishes (`--vw-eff` × `--vh-eff`),
// because the width class alone cannot say it: a phone on its side is `sm` by width, as a 1024 × 768 window is.
import { classifyViewport } from '../useViewport'

export type StreamLayout = 'header' | 'rail' | 'bars'

/** A window on its side and shorter than this takes the rail. A phone turned on its side is 320-430 px tall, less
 *  under its browser's own bar, and a header there took 42% of it (174 of 412 px, measured 2026-10-03); a browser
 *  on the smallest supported screen (768 px) still has about 650, where the header row is the better use of it. */
export const RAIL_BELOW_HEIGHT = 500

export function streamLayout(width: number, height: number): StreamLayout {
  if (width > height && height < RAIL_BELOW_HEIGHT) return 'rail'
  if (width <= height && classifyViewport(width) === 'xs') return 'bars'
  return 'header'
}

/** The rail's width in CSS px: stream.css's `--stream-rail`, the grid column beside the picture. */
export const RAIL_WIDTH = 120
/** The header's share of the page's height from which the picture under it counts as small: a short window, or one
 *  zoomed, where the row wraps (1280 x 720 at 125 %: 174 of 576 px). A 1080p browser's one-line row is 9 % of it. */
export const HEADER_SHARE_FOR_RAIL = 0.15

type Size = { width: number; height: number }
/** Whether a window the header layout leaves a small picture in shows the shack's screen larger beside the rail (the
 *  operator's pick, 2026-10-16). `page` is the page and `stage` the picture's region under the header row, both as laid
 *  out; `shape` is the shack's screen, width over height. Beside the rail the stage is the page's whole height and its
 *  width less the rail's, and the picture fits either stage at that shape. */
export function railShowsMore(page: Size, stage: Size, shape: number): boolean {
  if (!(page.width > RAIL_WIDTH && page.height > 0 && stage.width > 0 && stage.height > 0 && shape > 0)) return false
  if (page.height - stage.height < HEADER_SHARE_FOR_RAIL * page.height) return false
  const pictureWidth = (width: number, height: number) => Math.min(width, height * shape)
  return pictureWidth(page.width - RAIL_WIDTH, page.height) > pictureWidth(stage.width, stage.height)
}
