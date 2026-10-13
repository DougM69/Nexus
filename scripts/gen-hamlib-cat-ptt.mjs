#!/usr/bin/env node
// Generate the Hamlib "Can set PTT" table for every rig the picker offers.
//
// WHY. PTT Method CAT keys the radio with rigctld's `T 1`, and a backend that has no way to key
// the radio can still answer it. Hamlib 4.7.1's IC-706MKIIG (3011) declares `.ptt_type =
// RIG_PTT_NONE` and no `set_ptt`, and `rig_set_ptt` lets RIG_PTT_NONE through ("allowed for use
// with VOX and WSJT-X"), so `T 1` answers RPRT 0 while the radio stays on receive: Nexus shows TX
// and nothing goes out. The setup wizard switched PTT to CAT for any rig it could name, which put
// exactly these radios on a keying method that keys nothing. `NO_CAT_PTT_MODELS`
// (ui/src/rigFormChecks.ts) keeps the wizard off CAT for them and has Settings say so; this script
// is where that list comes from, from ONE line Hamlib states per model, `Can set PTT: Y|N`, so
// nobody re-types it.
//
// Source of truth: `rigctl --dump-caps -m <model>` from the BUNDLED Hamlib
// (`src-tauri/resources/hamlib`, 4.7.1), the build that ships in the installer. Model list: the two
// catalog tiers in `crates/tempo-audio/src/rigmodels.rs`, i.e. exactly the rigs the picker lists.
// Not the dump's `PTT type:` line: rigctl writes its own `-P` option into the caps before dumping,
// so every model reads `None` there (see `gen-hamlib-ptt-micdata.mjs`).
//
// A second fact per model, `transmitter`: whether the dump's `Rig type:` line carries the word
// `Transmitter` (the flags Hamlib prints there, `Other Receiver Transmitter Transceiver` for an
// IC-7300). A radio that cannot key over CAT but does transmit needs another way to key, so the
// setup wizard asks how the interface keys it (`NO_CAT_PTT_TRANSCEIVERS`). The Malachite DSP
// (2049) reads `Other Receiver`: a receiver, with nothing to key and nothing to ask.
//
// Output (deterministic, no timestamp, so regenerating an unchanged tree is a no-op diff):
//   ui/src/components/__fixtures__/hamlibCatPtt.json
//     { hamlib, catalog, rigs: [{ model, name, setPtt, transmitter }, …], missing, needsDaemon }
//   `rigFormChecks.catptt.test.ts` pins `NO_CAT_PTT_MODELS` to exactly the rigs with
//   `setPtt: false`, and `NO_CAT_PTT_TRANSCEIVERS` to those of them with `transmitter: true`. NET
//   rigctl (model 2) dumps nothing without a live daemon and lands in `needsDaemon`; its keying
//   belongs to the rigctld it reaches, so it is never on either list.
//
// Run:  node scripts/gen-hamlib-cat-ptt.mjs [path/to/rigctl]
//   Default binary is the bundled `rigctl.exe`, which runs under WSL interop on the dev box; pass a
//   native `rigctl` (Hamlib 4.7.1) elsewhere. Re-run and diff after any Hamlib bump or catalog
//   change: a row that moves is a rig whose CAT keying just appeared or went away.

import { readFileSync, writeFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const rigctl = process.argv[2] ?? join(repo, 'src-tauri/resources/hamlib/rigctl.exe')
const catalog = join(repo, 'crates/tempo-audio/src/rigmodels.rs')
const out = join(repo, 'ui/src/components/__fixtures__/hamlibCatPtt.json')

// The same catalog pattern `gen-hamlib-xit.mjs` reads: `(model, "Friendly name"),` rows anchored
// at line start, so a number inside a comment or a name cannot be mistaken for one.
const models = [...readFileSync(catalog, 'utf8').matchAll(/^\s*\(\s*(\d+)\s*,/gm)]
  .map((m) => Number(m[1]))
const uniq = [...new Set(models)].sort((a, b) => a - b)
if (uniq.length < 100) throw new Error(`only ${uniq.length} models parsed from ${catalog} — check the pattern`)

let hamlib = ''
const rigs = []
const missing = []
const needsDaemon = []
for (const model of uniq) {
  // As in the XIT script: a non-zero exit is normal for NET rigctl and for a model this Hamlib
  // does not carry, so keep what it wrote and let the missing caps header mark it.
  let caps = ''
  let err = ''
  try {
    caps = execFileSync(rigctl, ['-m', String(model), '--dump-caps'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch (e) {
    caps = e.stdout ?? ''
    err = e.stderr ?? ''
  }
  if (!caps.includes('Caps dump for model')) {
    ;(err.includes('Unknown rig num') ? missing : needsDaemon).push(model)
    continue
  }
  hamlib ||= caps.match(/^Hamlib version:\s*(.+)$/m)?.[1].trim() ?? ''
  const name = `${caps.match(/^Mfg name:\s*(.+)$/m)?.[1].trim() ?? ''} ${caps.match(/^Model name:\s*(.+)$/m)?.[1].trim() ?? ''}`
  const setPtt = caps.match(/^Can set PTT:\s*([YN])\b/m)
  if (!setPtt) throw new Error(`no "Can set PTT:" line in the caps dump for ${model} — the format moved`)
  const rigType = caps.match(/^Rig type:(.*)$/m)
  if (!rigType) throw new Error(`no "Rig type:" line in the caps dump for ${model} — the format moved`)
  rigs.push({
    model,
    name: name.trim(),
    setPtt: setPtt[1] === 'Y',
    transmitter: /\bTransmitter\b/.test(rigType[1]),
  })
}

writeFileSync(
  out,
  JSON.stringify({ hamlib, catalog: uniq.length, rigs, missing, needsDaemon }, null, 2) + '\n',
)
const noPtt = rigs.filter((r) => !r.setPtt)
console.log(`${out}: ${rigs.length} rigs dumped, ${noPtt.length} with Can set PTT: N (${hamlib})`)
console.log(`NO_CAT_PTT_MODELS: [${noPtt.map((r) => r.model).join(', ')}]`)
console.log(`NO_CAT_PTT_TRANSCEIVERS: [${noPtt.filter((r) => r.transmitter).map((r) => r.model).join(', ')}]`)
if (needsDaemon.length) console.log(`present but needs a live daemon to dump: ${needsDaemon.join(' ')}`)
if (missing.length) console.log(`not in this Hamlib: ${missing.join(' ')}`)
