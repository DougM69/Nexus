/**
 * The amplifier address field holds either a serial device (`COM8`, `/dev/ttyUSB2`) or, for a
 * KPA1500 on the network, `host:port`. One string, so a profile carries it unchanged and no
 * new setting needs migrating.
 *
 * The rule is `kpa_network_target` in `crates/tempo-audio/src/amplifier.rs`, and the two must
 * agree: a serial device never holds a `/` or `\` and a colon followed by a port number, so a
 * value that does is a network address. An IPv6 host is bracketed: `[fe80::1]:1500`.
 */

/** The KPA1500's command server port, unless the operator changed it with `^CP`. */
export const KPA_NET_PORT = 1500

/** Does this amplifier address name the network rather than a serial device? */
export function ampPortIsNetwork(value: string): boolean {
  const s = value.trim()
  if (/[/\\]/.test(s)) return false
  const m = /:(\d+)$/.exec(s)
  if (!m) return false
  const port = Number(m[1])
  return port >= 1 && port <= 65535
}

/** Split a network address into the host and port the two Settings fields show. */
export function splitAmpAddress(value: string): { host: string; port: number } {
  const s = value.trim()
  const i = s.lastIndexOf(':')
  const host = s.slice(0, i)
  const port = Number(s.slice(i + 1))
  return {
    host: host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host,
    port: Number.isInteger(port) && port >= 1 && port <= 65535 ? port : KPA_NET_PORT,
  }
}

/**
 * Join the two fields back into one address. Slashes and spaces are dropped from the host, so a
 * pasted `http://` cannot turn the address into something that reads as a serial device, and a
 * port outside 1–65535 falls back to 1500.
 */
export function joinAmpAddress(host: string, port: number): string {
  const h = host.replace(/[/\\\s]/g, '')
  const p = Number.isInteger(port) && port >= 1 && port <= 65535 ? port : KPA_NET_PORT
  return `${h.includes(':') ? `[${h}]` : h}:${p}`
}
