# JTTY

JTTY is the keyboard mode WSJT-X introduced in its 3.2 test releases: RTTY-style keyboard and
contest text, sent in 1.9-second frames of 4-tone FSK about 127 Hz wide, with error correction
that copies far below where RTTY gives up. There are no time slots; a station starts sending
whenever it likes, and the text prints as each frame decodes.

**This build receives JTTY. Its transmitter is built but switched off:** every send is refused
with "JTTY transmit is not switched on in this build", and nothing on this screen keys the rig
for JTTY. The dock at the bottom of the screen is one row, the compose bar with Send and
Esc/Stop, drawn disabled and saying so. WSJT-X's other transmit controls (the F1–F8 messages, the
Call box, the serial number and Live) appear above it only in a build with the transmitter
switched on.

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
- **Tx** — where JTTY would transmit: its lowest tone, the other three above it. As in WSJT-X,
  it goes back to 1500 Hz each time JTTY is opened from another mode. The license check judges
  the whole signal at this offset.
- **F Tol** — how far either side of Rx the decoder always searches, from WSJT-X's list
  (2 to 500 Hz). It is kept between sessions, as in WSJT-X; a new install starts at 20 Hz.

The decoder also searches the fixed 1200–1500 Hz and 1500–1800 Hz channels, but only where
they fall inside the waterfall's visible range, as WSJT-X searches them inside its wide graph.
Zooming the waterfall in narrows that range.

**The waterfall.** A green **RX** cursor marks Rx and a red **TX** cursor marks Tx; a click on
a signal moves Rx there, Shift+click (or a right-click) moves Tx, and Ctrl+click moves both.

**The TX strip** under the waterfall carries **TX On / TX Off**, **Tune**, **ATU** and
**Stop TX**, as on every cockpit. Stop TX, and Esc, stop whatever is on the air.

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
