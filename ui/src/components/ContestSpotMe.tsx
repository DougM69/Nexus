// ⚠️ ON THE MIGRATED LIST's terms: every operator-visible string is a catalog key; the cluster line
// shown after a spot is the wire text itself.
//
// SPOT ME, ON THE CONTEST SCREEN: one DX cluster spot of the operator's own call on the station's dial,
// with a short comment ("<contest> CQ" until the operator edits it). A spot is public, so ONLY A PRESS
// POSTS: nothing here runs on a timer or an event, a press while one is on its way is the same press,
// and after a spot went the button waits out the cooldown and says how long. The station holds the
// same cooldown and reads the call and dial itself (`contest_spot.rs`); this is the button's half.
import { useEffect, useRef, useState } from 'react'
import { contestSelfSpot, type ContestSpotPosted } from '../api'
import { t } from '../i18n'
import { pushToast } from '../toast'

/** The comment a spot starts with: the contest's short name and CQ. */
export function defaultSpotComment(contestShort: string): string {
  return contestShort.trim() ? `${contestShort.trim()} CQ` : 'CQ'
}

/** The cooldown, in seconds, before the station has said (`contest_spot::COOLDOWN`). */
const COOLDOWN_SECS = 60

function refusal(err: unknown): string {
  const token = String(err)
  const cooling = /spotCooldown:(\d+)/.exec(token)
  if (cooling) return t('spotMe.refused.cooldown', { secs: Number(cooling[1]) })
  if (token.includes('spotNoCall')) return t('spotMe.refused.noCall')
  if (token.includes('spotNoCluster')) return t('spotMe.refused.noCluster')
  if (token.includes('spotOffBand')) return t('spotMe.refused.offBand')
  if (token.includes('spotNoContest')) return t('spotMe.refused.noContest')
  if (token.includes('spotNotAllowed')) return t('spotMe.refused.notAllowed')
  if (token.includes('spotRfOnly')) return t('spotMe.refused.rfOnly')
  return t('spotMe.refused.failed')
}

/** The line as the cluster got it (`format_dx_spot`). */
function spotLine(p: ContestSpotPosted): string {
  return ['DX', String(p.freqKhz), p.call, p.comment].filter(Boolean).join(' ')
}

export function ContestSpotMe({ contestShort }: { contestShort: string }) {
  const [comment, setComment] = useState(() => defaultSpotComment(contestShort))
  // Follow the contest until the operator types their own.
  const edited = useRef(false)
  useEffect(() => {
    if (!edited.current) setComment(defaultSpotComment(contestShort))
  }, [contestShort])
  const busy = useRef(false)
  const [sending, setSending] = useState(false)
  const [coolUntil, setCoolUntil] = useState(0)
  const [now, setNow] = useState(() => Date.now())
  const left = Math.max(0, Math.ceil((coolUntil - now) / 1000))
  useEffect(() => {
    if (coolUntil <= Date.now()) return
    const id = window.setInterval(() => {
      const at = Date.now()
      setNow(at)
      if (at >= coolUntil) window.clearInterval(id)
    }, 1000)
    return () => window.clearInterval(id)
  }, [coolUntil])

  const press = async () => {
    if (busy.current || left > 0) return
    busy.current = true
    setSending(true)
    try {
      const posted = await contestSelfSpot(comment)
      const at = Date.now()
      setNow(at)
      setCoolUntil(at + (posted.cooldownSecs || COOLDOWN_SECS) * 1000)
      pushToast(t('spotMe.posted', { line: spotLine(posted) }), 'success')
    } catch (e) {
      pushToast(refusal(e), 'error')
    } finally {
      busy.current = false
      setSending(false)
    }
  }

  return (
    <div className="contest-spot-me">
      <input
        type="text"
        className="contest-spot-comment"
        aria-label={t('spotMe.comment.label')}
        value={comment}
        maxLength={30}
        spellCheck={false}
        onChange={(e) => {
          edited.current = true
          setComment(e.target.value)
        }}
      />
      <button
        type="button"
        className="contest-spot-btn"
        title={t('spotMe.button.title', { secs: COOLDOWN_SECS })}
        disabled={sending || left > 0}
        onClick={() => void press()}
      >
        {left > 0 ? t('spotMe.cooling', { secs: left }) : t('spotMe.button')}
      </button>
    </div>
  )
}
