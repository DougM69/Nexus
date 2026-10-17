// JS8Call's word suggestions for the compose box ("Suggest alternative word choices for more
// efficient message transmission", on by default): which words to underline and what a right-click
// offers for one. The words are found here, as JS8Call's editor finds them (letters and digits,
// joined across an apostrophe or a full stop, an `@` kept with the word after it); whether a word
// is underlined and what it is offered are the station's answer from the JSC dictionary
// (js8::proto::suggest, through `js8WordCheck` / `js8WordSuggestions`). Native only: the Remote
// page has no compose box.
import { useEffect, useState } from 'react'
import { js8WordCheck } from './api'

export interface Js8Word {
  /** UTF-16 offsets into the text as typed. */
  start: number
  end: number
  /** Upper-cased, as JS8Call checks it. */
  word: string
}

const WORD = /@?[\p{L}\p{N}]+(?:['.][\p{L}\p{N}]+)*/gu

/** The words of `text`, as JS8Call's editor walks them. */
export function js8Words(text: string): Js8Word[] {
  const out: Js8Word[] = []
  for (const m of text.matchAll(WORD)) {
    const start = m.index ?? 0
    out.push({ start, end: start + m[0].length, word: m[0].toUpperCase() })
  }
  return out
}

/** The station's answers so far, by word (true: underlined). Bounded: cleared past CACHE_MAX. */
const answers = new Map<string, boolean>()
const CACHE_MAX = 4000
const NONE: ReadonlySet<string> = new Set()

/** The words of `text` the dictionary codes in pieces, as the station answers them; asked only
 *  for words it has not answered before, a moment after typing stops. Empty while `enabled` is
 *  off. A word not answered yet is not underlined. */
export function useJs8FlaggedWords(text: string, enabled: boolean): ReadonlySet<string> {
  const [, setAnswered] = useState(0)
  useEffect(() => {
    if (!enabled) return
    const ask = [...new Set(js8Words(text).map((w) => w.word))].filter((w) => !answers.has(w))
    if (ask.length === 0) return
    let live = true
    const timer = setTimeout(() => {
      void js8WordCheck(ask)
        .then((flags) => {
          if (answers.size > CACHE_MAX) answers.clear()
          ask.forEach((w, i) => answers.set(w, flags[i] === true))
          if (live) setAnswered((n) => n + 1)
        })
        .catch(() => {})
    }, 150)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [text, enabled])
  if (!enabled) return NONE
  return new Set(js8Words(text).filter((w) => answers.get(w.word) === true).map((w) => w.word))
}
