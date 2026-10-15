// The QRZ.com page for a callsign, the same from every button that opens one. The desktop opens
// what `qrz_url` (src-tauri) builds; the hosted Remote page's QRZ button and the DXpedition link
// build it here, in the browser. `__fixtures__/qrz-urls.json` holds both to one table, so the
// desktop and the browser cannot open different pages for one station.
//
// QRZ has one page per callsign, so only the callsign is kept: an APRS SSID (`PI1UTR-10`; a
// callsign never contains a `-`) and a D-STAR module letter after a space (`PI1UTR B`) name one
// of the station's transmitters and are dropped, and a portable form keeps its base call: the
// part with a digit, so `W1AW/PORTABLE` opens W1AW. NOT
// for aprs.fi, which tracks each SSID, and NOT `callsign.ts`'s `baseCall`, the engine's "same
// station?" rule.

/** The QRZ.com page `call` opens, or null when nothing callsign-shaped is left. Only letters and
 *  digits reach the URL, so a crafted string can never build another one. */
export function qrzUrl(call: string): string | null {
  // `\p{White_Space}` is exactly Rust's `char::is_whitespace`, so both cut at the same place.
  const station = call.replace(/^\p{White_Space}+/u, '').split(/[-\p{White_Space}]/u)[0]
  // The base call of a portable form is the part that looks like a callsign, one with a digit
  // (`W1AW/PORTABLE`, `K1A/QRP`), and among those the longest (`PJ4/K1ABC`, `K1ABC/P`), the LAST
  // of equal ones (Rust's `max_by_key`); with no digit anywhere, the longest. Measured after
  // cleaning: JavaScript counts UTF-16 units and Rust counts bytes, so measuring the raw text
  // could pick different parts.
  let base = ''
  for (const part of station.split('/')) {
    const clean = part.replace(/[^A-Za-z0-9]/g, '')
    const digit = /[0-9]/.test(clean), baseDigit = /[0-9]/.test(base)
    if ((digit && !baseDigit) || (digit === baseDigit && clean.length >= base.length)) base = clean
  }
  return base ? `https://www.qrz.com/db/${base.toUpperCase()}` : null
}
