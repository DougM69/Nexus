# Nexus 1.19.0

This release gets a club ready for the Illinois QSO Party on Sunday 18 October. The host now lists
every position's contacts as they arrive, every laptop's county board lights what the whole club has
worked, and the host names a laptop that sends the wrong county. A contact in a mode the party does
not allow, FT8 or FT4, scores nothing and says so in a small chip. The host keeps the club's call,
its entry class and the whole contest after **Contest over?**, a rehearsal stays out of the party,
and the county box will not log a county that is on no list. **Put every laptop in the club on
1.19.0 before the party, the host's too** (Upgrading, below).

On the Flex native client (Beta), the voice keyer plays in Phone with native DAX audio on, and your
paddle can take over from Nexus's CW. On Windows, Nexus follows your computer's light or dark
setting until you pick a theme, and Light takes the softer silver look. The CW, RTTY and PSK macro
keys open their editor from a right-click or Ctrl+Shift with the F-key, and **Send once** sends a
change without saving it. The rest are fixes, many from your reports: eQSL says why it refused a
contact, LoTW confirmations come in for every call on your account, one band is one band however
an import spelled it, F5 no longer reloads Nexus on Windows, and an ISS pass no longer retunes the
radio you are using for FT8.

## The short version

- **Club sync for the Illinois QSO Party:** a Club log on the host, the club's worked counties on
  every laptop's board, a warning when a laptop sends another county, and the club's call, class
  and contacts kept after Contest over? on the host.
- **A contact in a mode the contest bans scores nothing**, and a small **FT8 · no credit** chip
  replaces the yellow band and the long amber sentence.
- **At the Illinois QSO Party** the county box refuses a county on no list, PE and PEI (or NL and
  NF) count once, and the Contest screen names a blank Entry class or Email before you export.
- **Flex native client (Beta):** the voice keyer in Phone with native DAX audio, and your paddle
  over Nexus's CW (built, off until checked on a radio).
- **Theme:** on Windows Nexus follows the computer's light or dark setting until you pick one;
  Light has the silver look, and the old Light is the **Bright** theme.
- **Macro keys:** right-click a CW, RTTY or PSK key, or press Ctrl+Shift with its F-key, to change
  what it sends; **Send once** sends the change once and **Save** keeps it.
- **The rail's mode badge** names the contest you are running (ILQP, NYQP, WFD), not FIELD DAY.
- **Confirmations:** eQSL says why it refused a contact, uploads name Nexus, a nickname change no
  longer skips cards, and a card no longer marks your contact "QSL sent". LoTW confirmations come in
  for every call on your account. The Logbook marks a QRZ confirmation **Q**.
- **The Logbook and statistics:** one band whatever its letter case, USB and LSB count as SSB,
  Most-worked states counts every US contact, a search for usb or lsb finds your phone contacts, and
  sorting by country keeps each country together.
- **Also:** the setup wizard asks how your interface keys a radio Hamlib cannot key over CAT, F5
  and Ctrl+R no longer reload Nexus on Windows, a satellite pass no longer retunes the radio you are
  using on another screen, the ADIF exports carry the report you sent, JS8's switches light green
  only when they can transmit, QRZ opens the callsign without an APRS SSID, and the scope's ▲ dB
  reading no longer shoves the buttons beside it.

## Upgrading from 1.18

- **Club sync: put every laptop on 1.19.0 before the party, the host's too.** A laptop on 1.18.0
  still joins a 1.19.0 host, and the other way round, because club sync has not changed its version.
  But a laptop on 1.18.0 whose call is changed while it is joined still sends its contacts to the
  club's log, under the club's call, where a 1.19.0 laptop keeps them in its own log. A 1.18.0
  laptop also shows only its own contacts on the county board. Update every laptop, then check that
  each one's club chip reads Synced.
- **The theme on Windows.** A new install on Windows opens in System and follows Windows' light or
  dark setting. A computer that ran an earlier version keeps Dark, because those versions saved
  Dark for everyone who never picked: pick **System** in Settings ▸ Appearance ▸ Theme to follow the
  computer. Linux, the Raspberry Pi, macOS and the hosted Remote page still open Dark until you
  pick.
- **Light looks different.** It now has the pale silver look of the Silver chassis theme, which is
  gone from the gallery: if you had Silver chassis picked, you see Light, which looks the same. The
  brighter Light of earlier versions is the **Bright** theme in the gallery, unchanged.
- **FT8 and FT4 at the Illinois QSO Party score nothing.** They were scored like any other contact
  although the chip said no credit. They are still logged and still in the Cabrillo and ADIF files,
  as a dupe is, so a score you saw in 1.18.0 with FT8 contacts in it goes down.
- **The first LoTW download takes longer.** It asks for your whole confirmation history once, for
  every call on your account. You do not need to press Download everything again.
- **A right-click on a macro key opens its editor** and no longer opens the browser's menu. A plain
  click or F-key still sends the key.

## Club sync and the Illinois QSO Party

- **The host lists every position's contacts as they arrive.** Under the club board on the host's
  Contest screen, in its pop-out, in the logger window and in the board's own pop-out, a **Club
  log** lists the club's contacts, newest first: time, call, exchange, band, mode and the position
  that logged each. A repeat of a contact another position made first is marked as a dupe. The list
  is read the same way as Club ADIF and Club Cabrillo, so the list and the files agree. It shows the
  newest 100 and how many the club holds. Only the host has it, and it is not on the Remote page.
- **Every laptop's county or sections board shows what the whole club has worked.** A county, state
  or ARRL section lights on every laptop's board as soon as any position logs it. This laptop's own
  are marked ⌂ in the colour its name has in the club grid, apart from the green ✓ of another
  position, with both counts ("34/102 worked · 12 on this laptop"). Remove last takes a county off
  every board again. When the host removes a position, the other laptops' boards say Removed after
  its name within a heartbeat. NEEDS-BENCH: two laptops on a LAN.
- **The host names a laptop that sends another county.** A laptop set up on another county than the
  club's sends a wrong exchange for every station it works. The host's Contest screen names it, with
  what it sends and what the club sends ("CW tent sends KANE, and this club sends MCLN"), until it
  sends the club's county again. A state, province or section that differs is named the same way.
- **A contact in a mode the contest does not allow scores nothing.** FT8 and FT4 at the Illinois
  QSO Party, and the WSJT modes at Winter Field Day, add nothing to the score, the club line, the TV
  scoreboard or the Cabrillo CLAIMED-SCORE, and light no county on any board. They are still logged
  and exported. A small outlined **FT8 · no credit** chip says so beside the mode on the log line, in
  the Contest banner and in the FT header; hover it for the whole sentence.
- **The host keeps the club's call, class and contest after Contest over?** Contest over? puts the
  host laptop's own call back. From the moment Contest mode goes off, the Club Cabrillo and Club
  ADIF keep the club's call and entry class, through a restart. The host still lets in laptops on
  the club's call, the TV scoreboard keeps the club's call, and a host reopened days later still has
  the whole contest.
- **A contact logged on another call stays out of the club's log.** A contact logged on the host,
  or on a laptop whose call was changed while it was joined, stays in that laptop's own log, and the
  club block says how many. At ARRL Field Day the GOTA station still sends its contacts under its
  own call. A laptop on an earlier version does not hold them back (Upgrading, above).
- **Queued contacts still reach the host after Contest mode goes off.** A laptop that lost its link
  with contacts still to send sends them when the link is back, with the mode on or off.
  **Contest over?** says how many the club's log does not have yet.
- **A rehearsal under the same Event name stays out of the party.** The host leaves out everything
  from an earlier running of the contest. A running begins a day before the contest starts, so a
  test contact made that day still counts unless you take it back with Ctrl+D twice.
- **The county box no longer logs a county that is on no list.** From a station in the USA or
  Canada, a value such as COKO, DUP or IL gets a line under the box, and Log stays off until it holds
  a real one. A name or the start of one gets the codes it may mean (Washington is WASH or WA). Type
  a two-word county without its space (stclair). A DX station's country still logs as typed.
- **A Canadian province counts once, whichever spelling its station sent.** PE and PEI, or NL and
  NF, are one multiplier in the score, on the club line, in the Cabrillo and on the board.
- **The Contest screen names a blank Entry class or Email** before you export, with a button to
  Settings ▸ Contesting ▸ Your entry. The file still exports either way.
- **The manual's club checklists gain three lines:** no test contacts from 1700Z on Saturday, every
  laptop's club chip reads Synced before anyone turns Contest mode off, and the host exports the
  Club Cabrillo before Contest over?, while the button is still on the Contest screen.

## Contesting

- **The rail's mode badge names the contest you are running**: NYQP, ILQP, WFD, CQ WW CW, JAN VHF,
  and FIELD DAY for ARRL Field Day. The torn-off scoreboard says no contest is running where it said
  Field Day was not active.
- **The contest logger window says to log a satellite pass's contacts on the Satellites screen**,
  while a transponder is picked there, so a pass contact is not logged as an ordinary 2 m or 70 cm
  one. It blocks nothing.
- **The contest logger window logs a JS8 contact as JS8**, and each digital mode as itself, where
  it named no mode and the contact went into the contest log as FT8.
- **With the logger window open, a contact you just logged no longer comes back** when one person
  types at the moment the other presses Enter.
- **What you type for the next station straight after Enter now stays**, with the logger window
  open or closed.
- **The log line's reports follow the mode unless you typed them**, so on the Satellites screen an
  FM pass and then a CW contact log 599 for the CW contact.
- **The Export ADIF and Club ADIF files carry the report you sent**, 59 on phone and 599 on CW and
  digital, as the Cabrillo line does.
- **"Contest over?" keeps your own settings through a rehearsal.** A change is noted from a week
  before the contest you picked until it ends, or while Contest mode is on, so an everyday change
  made between contests is no longer offered back.

## FlexRadio

All of this is on the Flex native client (Beta), and none of it is checked on a Flex radio yet.

- **The voice keyer plays in Phone with native DAX audio on.** Each message switches the radio to
  take its transmit audio from DAX for the message's own length, and gives the radio its mic back
  once the unkey is confirmed. A message the radio has not switched for within 2 seconds, or that
  something else keys first, is dropped and never played later; a message longer than 20 seconds is
  refused. Stop TX ends a message at once. Pressing the mic's PTT during a message still gets the
  amber alarm and a reconnect until a bench has shown what the radio reports for it. NEEDS-BENCH on
  a FLEX radio.
- **Your paddle wins over Nexus's CW**, built and switched off until a bench has shown what the
  radio reports for a paddle in its key jack. Then a paddle touch while Nexus sends CW clears the
  rest of the message, with no alarm and no reconnect, and the CW screen says your paddle took over.
  Until then a paddle touch gets the amber alarm and a reconnect, as in 1.18.0. Each failed check
  that a transmission ended writes one line to Nexus's diagnostic log, for the bench to send back.
  NEEDS-BENCH on a FLEX radio.
<!-- BENCH: the lead keeps ONE of the two paragraphs below, per item, after this week's bench steps.
     CW, Tune and ATU are decided separately; a mixed result keeps the "on" sentence for each that
     passed and the "off" sentence for each that did not, and the matching line under Known issues.

  IF THE BENCH PASSES (switched on):
- **CW, Tune and the radio's own ATU work through the Flex native client.** CW goes out through the
  client, so the CW screen no longer sends you to SmartSDR CAT; Tune keys the radio's own tune
  carrier; and the client offers an ATU button that starts the radio's tuner. Checked on a FLEX
  radio this week.

  IF THE BENCH DOES NOT PASS (still off):
- **CW, Tune and the radio's own ATU through the Flex native client stay switched off** until they
  have been checked on a radio. The CW screen still says the client sends no CW and that SmartSDR
  CAT does (turn the client off for CW, or use the WinKeyer or Soundcard keyer), Tune keys the radio
  and plays Nexus's tone, and the client offers no ATU button. NEEDS-BENCH on a FLEX radio.
-->

## Your screen

- **On Windows, Nexus follows your computer's light or dark setting until you pick a theme**, and
  changes when Windows does. Settings ▸ Appearance ▸ Theme ▸ System is there on every computer.
  NEEDS-BENCH: Windows, switched between light and dark.
- **Light has the softer silver look**: a pale silver page, white panels, graphite text and steel
  blue keys, by day and with Night on. The transmit red, the ON AIR sign and the signal and Needed
  colours are as they were.
- **Change what a CW, RTTY or PSK macro key sends on the fly.** Right-click a key, or press
  Ctrl+Shift with its F-key (Ctrl+Shift+F3 for F3), and its editor opens with the cursor in the
  message; hovering a key and clicking ✎ opens it too, now on CW as on RTTY and PSK. Neither sends anything. **Send once** sends
  the message as you changed it, one time, through the same checks the key itself uses, and the key
  keeps its own message; **Save** keeps your change on the key. On CW, a profile still on the
  built-in keys becomes your own set the first time you save one. A plain click or F-key still sends
  the key, and Esc with the editor open still stops an over on the air.
- **The scope's ▲ dB reading no longer shoves G, Z and the buttons beside it** on the Phone and CW
  scopes and the RF scope pane.
- **JS8's switches light green only when they can transmit now.** HB ACK, AUTOREPLY, RELAY, HB and
  the repeating CQ stay amber when they cannot, and say why when you point at one. When and what
  Nexus transmits is unchanged.
- **F5, Ctrl+F5 and Ctrl+R no longer reload Nexus on Windows**, in any Nexus window. Ctrl+P no
  longer prints and Ctrl+F no longer opens a find bar. Copy, paste and Nexus's own F-keys work as
  before. NEEDS-BENCH: Windows.

## Radios and satellites

- **The setup wizard asks how your interface keys a radio Hamlib cannot key over CAT**: the Icom
  IC-706, IC-706MKII, IC-706MKIIG, IC-718, IC-725, IC-726, IC-728, IC-729, IC-735 and IC-756, the
  Ten-Tec TT-585 Paragon and the Alinco DX-77. With PTT on CAT, Nexus showed TX while the radio
  stayed on receive. Pick VOX, or Serial RTS or DTR, and **Prove TX** beside the choice keys the
  radio for about two seconds so you can see it transmit. A Digirig Mobile or RIGblaster it
  recognises answers for you. Nothing you saved before changes until you pick. NEEDS-BENCH: one of
  these radios, keyed by VOX and by a serial line.
- **A satellite pass no longer retunes the radio you are using on another screen.** With ISS SSTV
  auto-arm on, every ISS pass tuned the radio to 145.800 FM wherever you were, so an FT-710 on FT8
  had transmit turned off and its decodes cleared. The auto-arm now tunes only while the SSTV or
  Satellites screen is open; elsewhere it tells you the ISS is up and leaves the radio alone.
- **ISS SSTV auto-arm on a radio that cannot receive 145.800 MHz says so once** per session, with
  where to turn it off (Settings ▸ Digital ▸ SSTV), and then leaves the radio alone.
- **Picking a satellite your only radio cannot receive no longer takes over the dial.** The pick
  says the radio does not cover it and holds nothing. With a second radio, the satellite is held as
  before.
- **QRZ opens the station's own callsign page**, without an APRS SSID (PI1UTR-10 opens PI1UTR) or a
  D-STAR module letter, in the app and on Remote. The aprs.fi link still opens that exact station.

## Logging and confirmations

- **One band is one band, whatever its letter case.** An import's `20M` and Nexus's `20m` are one
  band in the statistics and on the QSO globe. A phone contact whose sideband was written as its
  mode is held as SSB with the sideband as its submode, so USB and LSB count as SSB. The Logbook
  shows a contact's country as its DXCC entity, with the imported text on the tooltip. No award
  credit changes.
- **Most-worked states counts every US contact**, however LoTW, DXKeeper or another logger spelled
  the country, by the DXCC entity the WAS award counts it under. The Remote page counts the same way.
- **Searching the Logbook for usb or lsb finds your phone contacts**, and ssb finds all of them.
- **Sorting the Logbook by country keeps each country together**, by the country the Logbook shows.
- **eQSL says why it refused a contact.** A contact outside your eQSL account's dates no longer
  reads as a bad login, a missing or misspelled QTH Nickname is named as such, and the reason stays
  on the contact after the toast has gone.
- **eQSL uploads name Nexus**, as eQSL asks.
- **Changing your eQSL QTH Nickname no longer skips cards**: the next sync reads that account's
  whole InBox.
- **A card from eQSL no longer marks your contact "QSL sent"**, and a listener's (SWL) report is no
  longer read as a card. Contacts that already carry a copied mark keep it until you clear it from
  the row's QSL menu.
- **The Logbook's QSL column marks a QRZ confirmation Q**, beside L, C and E, and the eQSL tooltips
  say which awards accept eQSL.
- **LoTW confirmations come in for every call on your LoTW account**, each on the contacts made under
  its own call. A portable suffix (/P, /M, /MM, /AM, /QRP) counts as the plain call; a prefix or a
  call area's /9 stays a different call. Check confirmations says how many confirmations were for a
  call with no contacts in your log.
- **The manual's eQSL pages match the program.**

## Known issues

- Every laptop in a club needs 1.19.0: a laptop on 1.18.0 that changes its call while joined still
  sends its contacts to the club's log.
- The host's Club log is on the host's own screen only: a joined laptop has none, and it is not on
  the Remote page. The county-mismatch warning is on the host's screen only too.
- The host sees what a laptop sends only once it has logged a contact.
- The hosted Remote page shows the station's own county board, not the club's.
- NEEDS-BENCH, not yet checked on the hardware they need: the Flex native client's voice keyer and
  paddle stop (a FLEX radio), F5 and the theme on Windows, the wizard's keying question (one of the
  radios it names) and the club-wide county board (two laptops on a LAN).
<!-- BENCH: if CW, Tune or ATU stays off, keep this line, naming those that stay off:
- CW, Tune and the ATU through the Flex native client stay switched off until they are checked on a
  radio.
-->
- At the Illinois QSO Party and the other QSO parties, N3FJP gets the call, band, mode and frequency
  from Nexus, not the county. The Nexus log and its Cabrillo file are complete. (carried from 1.18.0)
- The Icom network connection receives and controls only: nothing transmits on it and there is no
  audio over it yet. (carried from 1.18.0)

**49 changes since 1.18.0.** The full list is in the CHANGELOG.
