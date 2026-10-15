# Nexus 1.19.0

This release gets a club ready for the Illinois QSO Party on Sunday 18 October. The host now lists
every position's contacts as they arrive, every laptop's county board lights what the whole club has
worked, and the host names a laptop that sends the wrong county. A contact in a mode the party does
not allow, FT8 or FT4, scores nothing and says so in a small chip. The host keeps the club's call,
its entry class and the whole contest after **Contest over?**, a rehearsal stays out of the party,
the county box will not log a county that is on no list, and a contest log that cannot be saved to
disk says so in red. **Put every laptop in the club on 1.19.0 before the party, the host's too**
(Upgrading, below).

FT8 and FT4 now transmit on WSJT-X's timing. An over ends where WSJT-X ends it, its tones sit on the
slot clock so a late key loses its first symbols and still ends on time, a late start follows
WSJT-X's 75 % rule, and no over is keyed without transmit audio. DXCC, WAS and WAZ count each
contact by the DXCC number and CQ zone its record carries, LoTW's first, so a DXpedition on a
US-style call such as K1N counts as Navassa Island, and the DXCC card shows the current count beside
the total. Statistics has a Timeline of your contacts by hour, day or month, and the Phone, CW and
Satellites log entry keeps what you typed when you switch pages.

On the Flex native client (Beta), the voice keyer plays in Phone with native DAX audio on. On
Windows, Nexus follows your computer's light or dark setting until you pick a theme, and Light
takes the softer silver look. The CW, RTTY and PSK macro keys open their editor from a right-click
or Ctrl+Shift with the F-key, and **Send once** sends a change without saving it. The rest are
fixes, many from your reports: two radios keep their own port, baud and sound card, the ATU button
starts an FT-710's tuner, eQSL says why it refused a contact, LoTW confirmations come in for every
call on your account, one band is one band however an import spelled it, F5 no longer reloads
Nexus on Windows, and an ISS pass no longer retunes the radio you are using for FT8.

## The short version

- **Club sync for the Illinois QSO Party:** a Club log on the host, the club's worked counties on
  every laptop's board, a warning when a laptop sends another county, and the club's call, class
  and contacts kept after Contest over? on the host.
- **A contact in a mode the contest bans scores nothing**, and a small **FT8 · no credit** chip
  replaces the yellow band and the long amber sentence.
- **At the Illinois QSO Party** the county box refuses a county on no list, PE and PEI (or NL and
  NF) count once, and the Contest screen names a blank Entry class or Email before you export.
- **A contest log that cannot be saved says so** in red on the Contest screen and the log strip,
  with when it last saved and a reminder to export a Cabrillo file.
- **FT8 and FT4 on WSJT-X's timing:** an over ends where WSJT-X ends it (the radio unkeys 13.84 s
  into an FT8 slot and 6.24 s into FT4), its tones sit on the slot clock, a late Call CQ keys up to
  75 % of the way through your slot, and no over is keyed without transmit audio.
- **DXCC, WAS and WAZ by each contact's own DXCC number**, LoTW's first: a K1N contact counts as
  Navassa Island, the DXCC card shows the current count beside the total, Sicily counts as Italy,
  and Awards lists the contacts where LoTW and your log disagree.
- **The Needed board and the statistics count each contact the way Awards does.**
- **A Timeline in Statistics:** your contacts over the past day, 30 days or year, one line per mode.
- **The log entry keeps what you typed** in Phone, CW and Satellites when you switch pages.
- **Two radios keep their own settings:** each radio's port, baud, sound card and drive stay its
  own in Settings ▸ Radio, and a Save keeps the drive you last set with the cockpit's Pwr slider.
- **Flex native client (Beta):** the voice keyer in Phone with native DAX audio, and your paddle
  over Nexus's CW (built, off until checked on a radio).
<!-- PENDING: feat/flex-unbenched-starts-setting-2026-10-14 -->
- **Flex native client (Beta): a switch, off by default,** lets the client send CW, run Tune and
  run the ATU before they are checked on a radio.
- **Theme:** on Windows Nexus follows the computer's light or dark setting until you pick one;
  Light has the silver look, and the old Light is the **Bright** theme.
- **Macro keys:** right-click a CW, RTTY or PSK key, or press Ctrl+Shift with its F-key, to change
  what it sends; **Send once** sends the change once and **Save** keeps it.
- **The rail's mode badge** names the contest you are running (ILQP, NYQP, WFD), not FIELD DAY.
<!-- PENDING: fix/cq-claimed-score-2026-10-14 -->
- **CQ WW and CQ WPX logs claim their score**, apart from a single-band entry with contacts on
  another band.
- **The Rotor box draws the station in your log entry on its rose** and says how far the antenna
  is off it.
<!-- PENDING: fix/yaesu-alc-scale-2026-10-14 -->
- **The FT-991A's and FT-710's ALC meter** reads on the radio's own scale.
- **Confirmations:** eQSL says why it refused a contact, uploads name Nexus, a nickname change no
  longer skips cards, and a card no longer marks your contact "QSL sent". LoTW confirmations come in
  for every call on your account. The Logbook marks a QRZ confirmation **Q**.
- **The Logbook and statistics:** one band whatever its letter case, USB and LSB count as SSB,
  Most-worked states counts every US contact, a search for usb or lsb finds your phone contacts, and
  sorting by country keeps each country together.
- **Also:** the setup wizard asks how your interface keys a radio Hamlib cannot key over CAT, the
  ATU button starts an FT-710's tuner, F5 and Ctrl+R no longer reload Nexus on Windows, a satellite
  pass no longer retunes the radio you are using on another screen, the ADIF exports carry the
  report you sent, JS8's switches light green only when they can transmit, QRZ opens the callsign
  without an APRS SSID, the scope's ▲ dB reading no longer shoves the buttons beside it, the manual
  says what is true in this version, and the TLS library behind Nexus's downloads is on its
  security release.

## Upgrading from 1.18

- **Club sync: put every laptop on 1.19.0 before the party, the host's too.** A laptop on 1.18.0
  still joins a 1.19.0 host, and the other way round, because club sync has not changed its version.
  But a laptop on 1.18.0 whose call is changed while it is joined still sends its contacts to the
  club's log, under the club's call, where a 1.19.0 laptop keeps them in its own log. A 1.18.0
  laptop also shows only its own contacts on the county board. Update every laptop, then check that
  each one's club chip reads Synced.
- **FT8 and FT4 transmit on WSJT-X's timing.** In FT8 the radio unkeys 13.84 s into the slot (it
  was about 13.39 s), which still leaves 1.16 s before the next slot. In FT4 it unkeys at 6.24 s,
  about 0.56 s sooner than before. A Call CQ or double-click late in your own slot keys at once up
  to 11.25 s into FT8 and 5.6 s into FT4, and sends the rest of the over. If the sound card's
  transmit output will not open when you turn TX on, the radio is not keyed and TX turns off, with
  the reason on the status line.
- **DXCC, WAS and WAZ totals can move.** Each contact counts by the DXCC number and CQ zone its
  record carries, and by LoTW's number where LoTW confirmed it under another. DXCC can go up (a K1N,
  K1B or K3J contact no longer counts as the United States), WAS can gain or lose a state, and WAZ
  can move where your log's CQ zone differs from the call's. Sicily and the other WAE areas count as
  the DXCC entity they belong to, and a record whose DXCC number is 0 counts for no entity, which is
  what 0 means in ADIF. Your first LoTW download on 1.19.0 asks for your whole history (below), so
  every contact LoTW confirms takes LoTW's DXCC number for the count, and its CQ zone where your log
  has none. Your log's own country is never changed.
- **Two radios: check each one's port, baud and sound card.** Earlier versions could put one
  radio's port, baud, sound card or drive on the other in Settings ▸ Radio. If yours ended up
  sharing one, set each radio again there.
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
<!-- PENDING: fix/cq-claimed-score-2026-10-14 -->
- **1.18.0 can over-claim a single-band CQ WW or WPX entry.** This release's contest rules add the
  claimed score to CQ WW and CQ WPX, and 1.18.0 takes them in through **Check for rules updates** on
  the Contesting tab (they apply when Nexus starts again). On 1.18.0, a log entered as one band that
  also holds a contact on another band then claims the score of every band, more than CQ credits,
  as 1.18.0 already does in CQ WW RTTY. 1.19.0 writes no claimed score for such a log. CQ rescores
  every log, so only the claimed number is wrong. Update before CQ WW SSB on 24 and 25 October.

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
- **A contest log that cannot be saved to disk says so.** When the contest journal cannot be
  written (a full disk, or a file held by another program), the contacts since the last good save
  are in memory only, and a power cut would lose them. The Contest screen and the contest log strip
  show a red line saying contacts are not being saved, since when, and to export a Cabrillo file
  now. It goes away as soon as a save works again, and that save carries every contact the failed
  ones missed.
<!-- PENDING: fix/cq-claimed-score-2026-10-14 -->
- **CQ WW CW and SSB and CQ WPX CW and SSB logs claim their score.** The Cabrillo file carries
  CLAIMED-SCORE, the score the Contest screen shows, as the CQ WW RTTY file already did. A
  single-band entry (Band category set to one band) whose log also holds a contact on another band
  writes no claimed score, in these four and in CQ WW RTTY, because CQ scores only the declared band
  and the screen counts every band. The line comes with this release's contest rules, which earlier
  versions take in as well (Upgrading, above, for 1.18.0). The guide's note on logging the
  Scandinavian Activity Contest as CQ WPX again says to delete the claimed score before you send
  the log.

## FT8 and FT4

- **An over ends where WSJT-X ends it.** In FT8 the audio stops 13.64 s into the slot and the radio
  unkeys at 13.84 s. It unkeyed at about 13.39 s, 0.45 s before WSJT-X, which left half the room for
  the sound card's delay. In FT4 the audio stops at 6.04 s and the radio unkeys at 6.24 s, about
  0.56 s sooner than before. FT4 keeps its tones 0.5 s into the slot.
- **The tones sit on the slot clock, as WSJT-X places them.** Anything that made the key late (a
  CAT exchange at the start of the slot, a decode still running, a busy computer) moved every tone
  late by the same amount and cut the end of the over at the next boundary. A late key now skips
  the first symbols, and the over still ends on time. An on-time over is unchanged: its tones start
  0.5 s into the slot.
- **A decode still running when your slot starts no longer delays your over.** Nexus builds the
  over it is about to send before that decode begins, and keys it on time.
- **A late start follows WSJT-X's 75 % rule.** A Call CQ or double-click inside your own slot keys
  at once while the slot is under 75 % of its length, up to 11.25 s into an FT8 slot or 5.6 s into
  FT4, and sends the symbols that remain. Its tones are on the slot clock and its PTT ends inside
  the slot (it ran 0.25 s into the next one). Later than that it waits for your next slot. The same
  limit holds when your slot's own key comes late (a decode that finished late, a busy computer):
  past 75 % Nexus does not key it, as WSJT-X does not, and the over goes out at your next slot.
- **An FT4 Call CQ with no tones left keys and holds, as WSJT-X does.** Between 5.54 s, where
  Nexus's FT4 tones end, and the 75 % limit at 5.625 s there is nothing left to send. Nexus keys and
  holds the transmitter, silent, until 6.24 s. Stop TX ends it at once.
<!-- PENDING: fix/ft-hold-rule-everywhere-2026-10-14 -->
- **Two more cases with no tones left hold the same way.** An FT4 slot whose own key comes between
  5.54 s and 5.625 s is keyed and held silent to 6.24 s. An FT8 over keyed in time whose radio comes
  on after its last tone (more than about 1.9 s late: split commands on a slow radio link, a busy
  computer) is held silent to 13.84 s.
- **An over is never keyed without transmit audio.** Nexus opens the sound card's transmit output
  when you turn TX on in the digital section (TX On, Call CQ, a double-click), so the first over of
  a session no longer goes out late by however long the open takes. If the output will not open
  (another program holds the device, or it is unplugged), the radio is not keyed, TX turns off, and
  the status line says why in the sound card's own words. Once the device is free, turn TX on
  again. Before, every over after a failed open keyed the radio with no audio for the rest of the
  session, and nothing said why. This covers FT8, FT4, JS8 and the other timed-slot modes. A
  station that only listens opens no transmit output, and Phone, CW and the keyboard modes open
  theirs as before.
<!-- PENDING: feat/ft-tx-encoder-lockfree-2026-10-14 -->
- **An over you change in the last seconds before your slot no longer waits for the decoder.**
  When what Nexus is about to send changes after the early decode has started (you move the TX
  frequency, double-click a station, call CQ or change the message, or the sequencer moves on),
  Nexus builds the over at the start of your slot. That build had to wait for the decode, so on a
  slow computer, a Raspberry Pi or a busy band the over could key late and lose its first symbols.
  Nexus builds FT8 and FT4 overs without waiting for the decoder, so they key on time, as WSJT-X's
  do. The tones are the ones Nexus sent before, checked against WSJT-X's own encoder for every kind
  of message, and a compound or hashed call you send still shows in full when the reply comes back.
  NEEDS-BENCH: beside WSJT-X on a slow computer, an over changed in the last seconds before your
  slot keys on time, and a QSO with a compound call completes.

Normal QSOs ran on the air with a test build of this timing. NEEDS-BENCH beside WSJT-X, on a dummy
load: a Call CQ about 10 s and about 12 s into an FT8 slot (the first keys at once, the second
waits for the next slot), an FT4 Call CQ about 5.6 s in (TX on with nothing sent, off at about
6.2 s), FT8 on a slow computer such as a Raspberry Pi, and TX On with the output device held by
another program.
<!-- BENCH: the paragraph above reflects the 14 October bench (normal QSOs on a test build with the
     timing and the 75 % rule, before the FT4 hold); update it after this week's bench of the
     whole set. -->

<!-- PENDING: js8-parity -->
## JS8: closer to JS8Call 3.0

- PLACEHOLDER: the JS8Call 3.0 parity changes, in operator words.

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
<!-- PENDING: feat/flex-unbenched-starts-setting-2026-10-14 -->
- **A Beta switch lets the client send CW, run Tune and run the ATU before they are checked on a
  radio. It is off by default.** **Settings ▸ Radio ▸ Rig & CAT ▸ Advanced ▸ Let Nexus send CW,
  Tune and ATU through the native Flex client (not yet bench-tested)** shows only while the Flex
  native client serves the radio (the client on and the radio's IP set). Off, the client works as
  in 1.18.0: it sends no CW (the CW screen says to turn the client off for CW, or to use the
  WinKeyer or Soundcard keyer), Tune keys the radio and plays Nexus's tone, and there is no ATU
  button. On, for that radio only, CW goes to the radio's keyer a word at a time, Tune is the
  radio's own carrier at its tune power, and an ATU button runs the radio's tuner when the radio
  reports one fitted. The switch lifts only the client's "not yet checked on a radio" refusal. TX
  on, your licence privileges, the TX watchdog and the client's own checks still decide every
  transmission: Nexus's slice must be the transmit slice with nothing else keying the radio, XIT
  off, and for CW break-in on and Sync CWX off. It changes only at the shack, never through the
  Remote stream, and saving a change reconnects Nexus to the radio. Try it into a dummy load first.
  NEEDS-BENCH on a FLEX radio, into a dummy load.
<!-- BENCH: the lead keeps ONE of the two paragraphs below, per item, after the Saturday bench of
     the Beta switch on the release build. CW, Tune and ATU are decided separately; a mixed result
     keeps the "checked" sentence for each that passed and the "not yet checked" sentence for each
     that did not, and the matching line under Known issues. If any passes, change "none of it is
     checked" at the top of this section to say what was. If the switch misses the tag, drop this
     block and keep the Known issues line in its "stay switched off" form.

  IF THE BENCH PASSES:
- **CW, Tune and the ATU through the client were checked on a FLEX radio into a dummy load**, with
  the Beta switch on. The switch stays off by default.

  IF THE BENCH DOES NOT PASS, OR IS NOT RUN:
- **CW, Tune and the ATU through the client are not yet checked on a radio.** Leave the Beta switch
  off unless you are trying it into a dummy load. NEEDS-BENCH on a FLEX radio.
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
- **The ATU button starts an FT-710's tuner.** On a Yaesu FT-710 the ATU button sent the command
  that starts a tune-up on other Yaesu radios, which the FT-710 does not use: the radio took it and
  nothing tuned, and in the digital modes Nexus still turned transmit off for the tune-up. Nexus
  sends the FT-710 its own tuning-start command, and every other radio gets the same command as
  before. NEEDS-BENCH: an FT-710 into a dummy load.
- **With two radios, each one keeps its own serial port, baud, sound card and drive.** Four things
  in Settings ▸ Radio could put one radio's settings on the other: **+ Add radio** kept the radio
  you had been editing in the form, **Edit** on a radio you had just saved showed what it had before
  the save, removing the radio you were editing left its settings in the form, and while you edited
  a radio not in use, its Tx Power and RX Gain sliders moved the radio in use. The form loads the
  radio it is editing every time and reads both radios again after each save, and + Add radio asks
  before it drops unsaved edits. While you edit a radio that is not in use, Tx Power and RX Gain
  set only that radio's saved levels, which it takes up when it becomes the radio in use, and the
  hint under them says so. NEEDS-BENCH: two Yaesus (an FT-710 and an FT-991A) on a Mac.
- **A Save in Settings keeps the drive and RX gain you set last.** After you moved the cockpit
  **Pwr** slider or changed RX Gain, Settings showed the level from before, and the next Save of
  anything in Settings set the radio in use back to it. The level you set last, in the cockpit or
  in Settings, is the one Settings shows and keeps and the one the radio in use gets. NEEDS-BENCH:
  an FT-710 into a dummy load.
- **The Rotor box shows on its rose where the station in your log entry is.** Beside a cockpit, the
  bearing on the box's second line is drawn on the rose as well: a dotted amber line from the middle
  with a small marker on the rim, under the antenna's own needle. The second line also says how far
  the antenna is off that bearing the short way round, for example "→ EC1DD 227° (1531 km) 20° off",
  so you can see whether to press Point. With no call in the entry, a station Nexus cannot place,
  or no grid of your own, there is no line and no number, and a rotator that does not report its
  position gets the line without the number. Nexus Remote is unchanged.
<!-- PENDING: fix/yaesu-alc-scale-2026-10-14 -->
- **The FT-991A's and FT-710's ALC meter reads on the radio's own scale.** Nexus showed the ALC of
  these two radios at four times the radio's own reading, and at 100 % in red from a quarter of the
  radio's ALC bar up, so a 991A run below full power, which raises its ALC reading by design, looked
  overdriven on every over. Nexus reads the radio's own ALC value and shows the same share of the
  bar as the radio. Every other radio's ALC reads as before. NEEDS-BENCH: an FT-991A into a dummy
  load, its ALC bar next to Nexus's at three drive levels.

## Awards and statistics

- **DXCC, WAS and WAZ count each contact by the DXCC number and CQ zone its record carries.** They
  went by the call alone, so a DXpedition on a US-style call counted as the United States: K1N
  (Navassa Island, 2015), K1B (Baker & Howland Islands, 2002), K3J (Johnston Island, 2001) and
  others like them. A contact counts by the DXCC number from its ADIF import or LoTW download, and
  by LoTW's number when LoTW confirmed it under another. A contact with no number counts by its
  call, as before. Your log's own country is never changed. The numbers come from AD1C's cty.csv
  and, for the deleted entities, the ARRL's TrustedQSL.
- **The DXCC card shows the current count beside the total**, for example "340 current · 349 total,
  deleted entities included". A deleted entity such as East Germany counts in the total only.
- **Awards lists the contacts LoTW counts as another entity.** Where LoTW confirmed a contact under
  a different DXCC entity than your log has, the contact is listed under **LoTW counts as another
  entity**, as "Your log: United States (291) · LoTW: Navassa Island (182)". The awards count
  LoTW's entity, and your log is not changed.
- **Sicily and the other WAE areas count as the DXCC entity they belong to**, Sicily as Italy. WAS
  counts a state by the contact's DXCC, so a KP4 call signing from Florida counts Florida, and WAZ
  counts the CQ zone the contact's record carries. A LoTW download or an ADIF import keeps the DXCC
  number and CQ zone it brings for a contact already in your log. The Remote page counts the same
  way.
- **The Needed board and the statistics count each contact the way Awards does.** The Needed
  board's worked and confirmed entities, the statistics' entity counts, Worked All Provinces and the
  First DX achievement read the same DXCC number, and the board's new-zone and new-state marks go by
  the CQ zone and DXCC number your contact carries. So the board does not call Navassa needed once a
  K1N contact counts for it, and a contact from Sicily counts toward Italy there too. A station on
  the air has no contact in your log yet, so the board still places it by its call.
- **A Timeline card in Statistics.** A line chart of your contacts over the past day (by hour), the
  past 30 days (by day) or the past year (by month), each ending with the current hour, day or month
  in UTC, as the log is. One line per mode, counted as the By mode card counts them (USB and LSB are
  SSB); past six modes, the five you worked most in that range keep a line each and the rest share
  one Other line. Each line has its own dash pattern and marker as well as its colour. Point at the
  chart, or focus it and use the arrow keys, to read each hour, day or month; a screen reader gets
  every count as a table. It opens on the past 30 days.

## Logging and confirmations

- **The log entry keeps what you typed when you switch pages.** A call typed into the Phone or CW
  cockpit's log entry, with its reports, name, QTH, comment, notes and park, is still there when you
  come back from another page such as Conditions, until you log it or clear it, and the Satellites
  log entry does the same. In CW the call is back in His Call too, so the F-keys send the call on
  screen. The band, frequency and mode are read from the radio when you log. Nothing is kept across
  a restart, in a contest's entry, or while "Log a contact from another radio" is open. The RTTY,
  PSK and JS8 cockpits already kept theirs.
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

## Also in this release

- **The manual, the FAQ and the README say what is true in this version:** the sections board's 85
  ARRL and RAC sections, FT8 and FT4's contest messages in ARRL Field Day and the ARRL VHF contests
  (WSJT-X's RTTY Roundup and WW Digi modes are still not in Nexus), the five languages Nexus ships
  in, and the Icom network connection and the Flex native client as Betas.
- **AD1C's country files carry their licence notice.** Nexus finds a call's DXCC entity and zones in
  AD1C's cty.dat, which is MIT-licensed, and its DXCC numbers in AD1C's cty.csv under the same
  licence. NOTICE, which every installer carries, has the notice in full, and the README credits
  AD1C. The list of deleted DXCC entities comes from the ARRL's TrustedQSL, and NOTICE carries its
  licence too.
- **Security: the TLS library behind Nexus's downloads is on its security release.** Nexus uses
  rustls for HTTPS (propagation data, maps and the other downloads); 0.23.46 fixes the four issues
  published on 14 October 2026 (RUSTSEC-2026-0346, 0347, 0348 and 0349). Nothing to do on your
  side.

## Known issues

- Every laptop in a club needs 1.19.0: a laptop on 1.18.0 that changes its call while joined still
  sends its contacts to the club's log.
- The host's Club log is on the host's own screen only: a joined laptop has none, and it is not on
  the Remote page. The county-mismatch warning is on the host's screen only too.
- The host sees what a laptop sends only once it has logged a contact.
- The hosted Remote page shows the station's own county board, not the club's.
- The Timeline card is not on a Remote observer's Statistics yet.
- The Logbook's Country column names the DXCC entity of the call, so a K1N contact reads United
  States there while Awards counts it as Navassa Island.
<!-- DROP IF LANDED: feat/ft-tx-encoder-lockfree-2026-10-14 -->
- An FT8 or FT4 over you change in the last seconds before your slot (a new TX frequency or
  message, a double-click) is built at the key and can still wait for a decode that is running, on
  a slow computer or a busy band, and then loses its first symbols.
- NEEDS-BENCH, not yet checked on the hardware they need: the FT8 and FT4 late-start limit, the FT4
  silent hold and the no-audio refusal (a dummy load beside WSJT-X), the Flex native client's voice
  keyer and paddle stop (a FLEX radio), the FT-710's ATU (an FT-710 into a dummy load), two radios'
  settings and the drive a Save keeps (an FT-710 and an FT-991A, on a Mac), F5 and the theme on
  Windows, the wizard's keying question (one of the radios it names) and the club-wide county board
  (two laptops on a LAN).
<!-- BENCH: if CW, Tune or ATU is not checked on the bench, keep this line, naming those that are
     not; if the Beta switch misses the tag, say instead that they stay switched off until checked:
- CW, Tune and the ATU through the Flex native client are not yet checked on a radio. The Beta
  switch that turns them on is off by default.
-->
- At the Illinois QSO Party and the other QSO parties, N3FJP gets the call, band, mode and frequency
  from Nexus, not the county. The Nexus log and its Cabrillo file are complete. (carried from 1.18.0)
- The Icom network connection receives and controls only: nothing transmits on it and there is no
  audio over it yet. (carried from 1.18.0)

**60 changes since 1.18.0.** The full list is in the CHANGELOG.
<!-- COUNT: the bullets under [Unreleased] at e3690bfb5 are 60. Recount at the tag: the rotor line,
     the encoder, the Beta switch, the CQ claimed score and the ALC each add a bullet; the hold rule
     and the Needed board extend existing bullets. This file is the GitHub release body and the
     update feed's notes as written, so no HTML comment may be left in it at the tag. -->

