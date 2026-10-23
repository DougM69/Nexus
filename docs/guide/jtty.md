# JTTY

JTTY is the keyboard mode WSJT-X introduced in its 3.2 test releases: RTTY-style keyboard and
contest text, sent in 1.9-second frames of 4-tone FSK about 127 Hz wide, with error correction
that copies far below where RTTY gives up. There are no time slots; a station starts sending
whenever it likes, and the text prints as each frame decodes.

JTTY comes from the WSJT-X 3.3.0-beta1 test release, and no WSJT-X general release has it yet:
the station you work needs a WSJT-X 3.2 or 3.3 test release. Nexus's JTTY transmitter has not yet
been tried on the air; check keying, power and ALC on a dummy load before you use it.

JTTY sits in the Keyboard section beside [PSK](psk.md): the same data privileges, the same
power cap and the same USB data mode. Opening the JTTY screen makes JTTY the section's modem,
and opening the PSK screen makes it PSK again.

## The tour

**The header.** A **JTTY** badge, the band picker and the frequency readout. The band picker
serves WSJT-X's JTTY frequencies, eleven of them from 160 m (1.839) to 2 m (144.160), with
**20 m at 14.090**; a channel where your license class may not send data is marked RX only.
Opening the JTTY screen from another mode moves the dial to JTTY's frequency on the band you
are on, if your class may send data there.

Beside them are the frequency controls WSJT-X has for JTTY:

- **Rx** — the audio frequency the decoder always searches. Type a value and press Enter, or
  click a signal on the waterfall. As in WSJT-X, it goes back to 1500 Hz each time JTTY is
  opened from another mode.
- **Tx** — where JTTY transmits: its lowest tone, the other three above it. As in WSJT-X, it
  goes back to 1500 Hz each time JTTY is opened from another mode. The license check judges the
  whole signal at this offset. It cannot move while anything is queued or on the air.
- **F Tol** — how far either side of Rx the decoder always searches, from WSJT-X's list
  (2 to 500 Hz). It is kept between sessions, as in WSJT-X; a new install starts at 20 Hz.

The decoder also searches the fixed 1200–1500 Hz and 1500–1800 Hz channels, but only where
they fall inside the waterfall's visible range, as WSJT-X searches them inside its wide graph.
Zooming the waterfall in narrows that range.

**The waterfall.** A green **RX** cursor marks Rx and a red **TX** cursor marks Tx; a click on
a signal moves Rx there, Shift+click (or a right-click) moves Tx, and Ctrl+click moves both.

**The TX strip** under the waterfall carries **TX On / TX Off**, **Tune**, **ATU** and
**Stop TX**, as on every cockpit. Stop TX, and Esc, stop whatever is on the air. Opening the JTTY
screen turns TX on, as opening PSK does, but not after a Stop: then TX stays off until you turn it
on.

**All decodes** prints every JTTY message, in the order the messages started, in WSJT-X's
own layout: the audio frequency, the SNR and the text, wrapped at 40 characters. A message
still arriving is dimmed until it ends. The head carries:

- **Arm RX / RX armed** — the decoder. It arms itself when you open the screen; switch
  **Start receiving when JTTY opens** off in [Settings ▸ Digital ▸ PSK](settings-reference.md#psk)
  to arm it by hand. Stopping it yourself is remembered for the session.
- **Lower case** and **Include time** — WSJT-X's two display options: messages in lower case,
  and the UTC time each message started in front of it. Both are kept.
- **Erase** — empties both panes. Stations already heard stay heard.

**QSO frequency** shows the messages first heard within F Tol of Rx: the station you are
listening to.

On a wide window the two panes stand side by side, as WSJT-X draws them, with the log strip
in a column of its own beside them. On a laptop screen they share the left column and the log
strip keeps the right. **⊞ Panels** in the header hides either pane or the waterfall, and
shows the radio's own scope.

If the receive audio drops out long enough for the decoder to lose its place, a line under the
TX strip says so and decoding starts again.

## Transmitting

The dock at the bottom of the screen is WSJT-X's JTTY transmitter.

- **The compose bar and Send** — type a message and press Enter or Send. It goes out as WSJT-X
  sends it: a line longer than 80 characters goes in pieces, and a line break ends a message.
  While a message is still going, Send shows how many pieces are left; another Send queues behind
  it and goes on the same transmission. `%M` is your call, `%H` the call in the Call box, `%E`
  your exchange, `%N` the serial and `%G` your grid.
- **F1 to F8** — WSJT-X's contest messages: `CQ %M CQ`, `%H %E`, `%H TU CQ %M CQ`, `%M`, `%H`,
  `TU NOW %Q %E`, `%H AGN?` and `%E`. Click one or press its key. A call in them must be a
  standard callsign, which JTTY sends in a compact form; a key whose call is not one is refused,
  never sent as text.
- **The Call box** — the other station. Clicking a confirmed call in a pane fills it.
- **The serial number** — what `%E` and `%N` send outside a contest, `001` and up. It moves on
  by one when a message starting `TU` goes out, as in WSJT-X, and is kept between sessions.
- **Live** — keeps the transmitter up while you type: your words go out as you finish them, and
  JTTY's filler holds the carrier between them. **Sign off** sends `DE <your call> K` and ends it.
  A live session ends at ten minutes whatever you are typing.
- **Esc/Stop** — cuts JTTY at once: what is on the air and everything queued.

Every send needs **TX On** and your license class across the whole signal at the Tx offset; a
send that cannot go says why under the dock. The **TX watchdog** stops JTTY when nothing has been
typed or sent for its whole limit (WSJT-X runs none for JTTY), and a queue too long for the
watchdog is refused before it keys. Stop TX, Esc, TX Off, the watchdog and leaving the JTTY
screen each stop JTTY and drop what is queued, and the line under the dock says why. Your own
transmission never counts as hearing a station.

## Contests

In **ARRL Field Day**, `%E` is your class and section, and F2, F6 and F8 send them in JTTY's
compact exchange form, as WSJT-X does in its Field Day mode. **Winter Field Day** allows JTTY
for 2027. Its classes (H, I, O and M) do not fit that form, so type the exchange out
(`W1AW 1O WI`): it goes as text, and a WSJT-X station reads it the same way. In any other
contest, `%E` is the serial number.

## Calls you can click

A call becomes clickable once it has been decoded in **two separate messages** within two
minutes. JTTY's frames carry a short checksum, so now and then a frame of noise decodes as
text, and some of those look like real calls. A real station repeats its call; a false decode
does not. So a call heard only once is plain text, and nothing looks it up.

Clicking a confirmed call puts it in the log strip's call box, and the log strip looks it up
in your callbook. The same rule keeps single decodes off PSK Reporter and off the
[Needed](needed-dx.md) board: a JTTY station appears there, labelled JTTY, once it has been heard
twice.

## Logging and LoTW

The log strip logs a JTTY contact as **JTTY**, the way WSJT-X writes it. TQSL does not know
the JTTY mode yet and would drop such a contact from an upload, so a LoTW upload holds JTTY
contacts back, leaves them marked as not uploaded, and says so on screen and in the
connection log.

## WSJT-X UDP

While the JTTY screen's section is in use, the WSJT-X UDP Status names the mode **JTTY**, as
WSJT-X does, and no Decode messages are sent: WSJT-X sends none for JTTY either, so a program
listening to Nexus sees the same as it would from WSJT-X.
