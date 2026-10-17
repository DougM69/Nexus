// Turn the FCC's public amateur licence files into the SQL that refreshes the Worker's keyed copy
// (`uls_calls`, 0013_subscription.sql), for the operator or a scheduled job to apply.
//
//   TRIAL_KEY=… node remote/scripts/uls-import.mjs --dir <unzipped l_amat or l_am_<day>> \
//     --mode full|daily --label <file date> --out <file.sql>
//   then: wrangler d1 execute <database> --remote --file <file.sql>
//
// It reads two files from the unzipped download, EN.dat (the licensee: call and email) and HD.dat (the
// licence: its status), and writes nothing in the clear: each call is the HMAC of its base call and
// each email the HMAC of its mailbox hash, both under TRIAL_KEY, exactly as trial-identity.ts and
// authority.ts form them. It contacts nothing and prints only counts; the key comes from the
// environment and is never echoed. Running it with the production key is credential handling.
//
// `full` replaces the whole copy (rows the new file no longer has are removed after the new ones land,
// so a reader never sees an empty table); `daily` upserts the day's changes. Both stamp `uls_sync`,
// whose age is what lets the Worker refuse US trials once the copy goes stale.
import { createHash, createHmac } from 'node:crypto'
import { readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

/** authority.ts `mailbox()`, the same rule: one person's aliases are one mailbox. A test holds the two
 *  together by value (a licence email proving a callsign for the matching sign-in address). */
export function mailbox(address) {
  const email = address.trim().toLowerCase(), at = email.lastIndexOf('@')
  if (at < 1) return email
  let local = email.slice(0, at), domain = email.slice(at + 1)
  if (local.indexOf('+') > 0) local = local.slice(0, local.indexOf('+'))
  if (domain === 'googlemail.com') domain = 'gmail.com'
  if (domain === 'gmail.com') local = local.replace(/\./g, '') || local
  return `${local}@${domain}`
}

const CALL = /^[A-Z0-9]{1,3}[0-9][A-Z0-9]{0,3}[A-Z]$/

/** One row per call: active when any of its licences is, and the active licensee's email from EN. Only
 *  licences HD.dat describes are emitted, since only those carry a status. Fields are 1-based in the
 *  FCC's layout: EN 2 = system id, 5 = call, 16 = email; HD 2 = system id, 5 = call, 6 = status. */
export function ulsRows(enText, hdText) {
  const emails = new Map()
  for (const line of enText.split(/\r?\n/)) {
    const field = line.split('|')
    if (field[0] === 'EN' && field[15]) emails.set(field[1], field[15].trim())
  }
  const calls = new Map()
  for (const line of hdText.split(/\r?\n/)) {
    const field = line.split('|')
    if (field[0] !== 'HD') continue
    const call = (field[4] ?? '').trim().toUpperCase()
    if (!CALL.test(call)) continue
    const row = calls.get(call) ?? { call, active: false, email: null }
    // Only an active licence's email is kept, none when it has none: an expired or reassigned call is
    // never proven by a previous holder's address.
    if (field[5] === 'A') { row.active = true; row.email = emails.get(field[1]) ?? null }
    calls.set(call, row)
  }
  return [...calls.values()]
}

const hmac = (key, value) => createHmac('sha256', key).update(value).digest('hex')

export function ulsSql(rows, key, mode, label, now) {
  if (!key) throw new Error('TRIAL_KEY is not set.')
  if (mode !== 'full' && mode !== 'daily') throw new Error('--mode must be full or daily.')
  if (!/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(label ?? '')) throw new Error('--label must be the file date, YYYY-MM-DD.')
  const values = rows.map(row => {
    const email = row.email ? hmac(key, `mailbox:${createHash('sha256').update(`email:${mailbox(row.email)}`).digest('hex')}`) : null
    return `('${hmac(key, `call:${row.call}`)}',${email ? `'${email}'` : 'NULL'},${row.active ? 1 : 0},${now})`
  })
  const statements = []
  for (let i = 0; i < values.length; i += 500) {
    statements.push(`INSERT INTO uls_calls(call_hmac,email_hmac,active,refreshed_at) VALUES ${values.slice(i, i + 500).join(',')}
ON CONFLICT(call_hmac) DO UPDATE SET active=excluded.active, email_hmac=excluded.email_hmac, refreshed_at=excluded.refreshed_at;`)
  }
  if (mode === 'full') statements.push(`DELETE FROM uls_calls WHERE refreshed_at<${now};`)
  statements.push(`INSERT INTO uls_sync(id,last_daily,last_full,synced_at) VALUES(1,${mode === 'daily' ? `'${label}'` : 'NULL'},${mode === 'full' ? `'${label}'` : 'NULL'},${now})
ON CONFLICT(id) DO UPDATE SET last_daily=COALESCE(excluded.last_daily, uls_sync.last_daily), last_full=COALESCE(excluded.last_full, uls_sync.last_full), synced_at=excluded.synced_at;`)
  return statements.join('\n') + '\n'
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const arg = name => { const at = process.argv.indexOf(`--${name}`); return at > 0 ? process.argv[at + 1] : undefined }
  const dir = arg('dir'), out = arg('out')
  if (!dir || !out) throw new Error('Usage: --dir <unzipped files> --mode full|daily --label YYYY-MM-DD --out <file.sql>')
  const rows = ulsRows(await readFile(join(dir, 'EN.dat'), 'latin1'), await readFile(join(dir, 'HD.dat'), 'latin1'))
  await writeFile(out, ulsSql(rows, process.env.TRIAL_KEY ?? '', arg('mode'), arg('label'), Date.now()))
  console.log(`${rows.length} calls, ${rows.filter(row => row.active).length} active, ${rows.filter(row => row.email).length} with an email`)
}
