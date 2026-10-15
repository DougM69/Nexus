# JS8

JS8 is keyboard-to-keyboard chat on FT8's waveform, on the air compatible with
[JS8Call](https://js8call.com): the same four speeds, the same heartbeats, directed
messages, relays and store-and-forward inbox, so a JS8Call station and a Nexus station
work each other without either knowing the difference. It is a ragchew mode, not a
contest mode — a short exchange takes a few periods, a paragraph takes a minute — and it
is the mode of choice on 40 and 20 m when the band is too poor for voice and FT8's
canned exchange is not a conversation.

**Before you start.** JS8 is on out of the box — look in the Digital group of the rail,
after APRS. If you don't operate it, switch it off in
[Settings ▸ Appearance ▸ Features](settings-reference.md#features). Entering the screen
tunes the rig to the JS8 watering hole for the current band (7.078, 14.078, 3.578 … —
JS8Call's own list, USB) and starts decoding every speed at once. Nothing transmits
until you enable TX in the header, every session. The mode's own switches — transmit
speed, which speeds to decode, heartbeat interval, heartbeat networking and its rules, the
three automatic-reply switches, the idle watchdog and your INFO/STATUS/group texts — are in
[Settings ▸ Digital ▸ JS8](settings-reference.md#js8).

## The tour

**The header.** A **JS8** badge, then the four speed chips — **Slow · Normal · Fast ·
Turbo** (30 / 15 / 10 / 6 second periods); the lit one is the speed *you* transmit at and
the period the TX clock follows. Beside them an **RX ▼ n/4** plate says how many of the
four speeds the receiver is decoding right now (all four by default). Then the band
picker with the JS8 watering holes, license-filtered per band; a **Drive** slider (TX
audio level — set it so the rig's ALC barely moves); **Tune**; an **ATU** button when your
rig reports a tuner; the **TX On / TX Off** latch; **Stop TX**; and **⊞ Panels**.

**The waterfall.** Click sets your RX offset, right-click your TX offset, Shift-click
both. JS8 stations sit anywhere from 500 to 2500 Hz; heartbeats cluster in the
500–1000 Hz sub-band by convention.

**The panes** (each removable from ⊞ Panels):

- **Activity** — every frame decoded at every enabled speed, one row each: UTC, the
  speed letter (E Slow · A Normal · B Fast · C Turbo, JS8Call's ALL.TXT letters), the
  audio offset, the SNR, and the message exactly as JS8Call would print it. Rows
  addressed to you carry a bar; your own transmissions are tinted; faint rows are
  low-confidence copy; italic rows closed without their last frame. Double-click a row
  to write to that station.
- **Stations** — everyone heard, with grid, SNR, offset, speed, age, and chips for a
  recent heartbeat, a recent CQ, and any messages you hold for them. Click a callsign to
  address it; the five quick buttons send **SNR? · GRID? · INFO? · HEARING? · QUERY MSGS**
  to that station, which answers by itself if its auto-reply is on. **Store…** at the end of a
  row leaves a message at your station for that station, as JS8Call's Store Message does: it
  waits in the Inbox, from your call, and goes out only when the station asks for it (QUERY
  MSGS, then QUERY MSG n). Storing it sends nothing.
- **Inbox** — messages addressed to you, and **MSG TO:** messages you are holding for
  someone else until they collect them (store and forward). Mark read, delete. A message held
  for one station stays until you delete it, as in JS8Call. One left for a group
  (`MSG TO:@GROUP`, a group you joined) is offered for 48 hours to every station that sends
  QUERY MSGS to the group, and handed to each of them once. The inbox keeps at most 100 messages
  (and 64 KB) of yours and as many held for others; when it is full, a message already handed
  over goes first, then the oldest, and a note at the top of the Inbox says which message went
  and why until you press **Clear notes**. A held message is handed over in one transmission,
  which holds at most 19 frames at Slow, 39 at Normal, 59 at Fast and 99 at Turbo; one that needs
  more is not offered or handed over until you change to a speed it fits, and its row says so and
  names that speed.
- **Log** — the log strip, prefilled from the station you last addressed. A JS8 QSO logs
  as `MFSK` / `JS8` in ADIF.
- **Boxes** — up to six panes of [Conditions](connect.md) among these, from ⊞ Panels ▸
  **Arrange** ▸ **+ Add a box** at the foot of a column, each closed by its own **✕** (see
  [Phone](phone.md)). Nothing in a box transmits.
- **Moving them** — ⊞ Panels ▸ **Arrange**'s arrows, or drag a pane or a box by its title
  bar onto a column or between two panes (see [Phone](phone.md)). The Log moves only up and
  down in its column, and nothing in the TX dock can be dragged.

**The TX dock**, pinned under the panes. The **To** box (blank = everyone, a callsign,
`@ALLCALL`, or a `@GROUP` you belong to), the composer, a frame estimate ("3 frames ·
45 s") and **Send**; a command palette with JS8Call's 32 directed commands; the **CQ**
variant picker and **CQ**; **HB NET** and, while it is on, **HB** with its menu (**▾**, or
right-click **HB**); the **AUTOREPLY · RELAY · HB ACK** chips; an automatic
reply's question, **Yes** or **No**; the queue with **Drop queue**; and the
idle-watchdog readout.

## Receiving

All four speeds decode at once, exactly as JS8Call's multi-decode does: a Slow station at
−26 dB and a Turbo station rattling off a paragraph on the same band both print. Untick
speeds in Settings if a small machine struggles. A long message arrives one frame per
period and is stitched together by audio offset — you see the frames arrive in Activity,
and the finished message in Inbox if it was for you.

## Transmitting

JS8 keys **one frame per period, every period**, with no even/odd parity — a 60-character
message at Normal is five frames, 75 seconds. **Send** queues the frames; the next period
boundary sends the first. Plain text (no addressee) goes out prefixed with your callsign,
so every frame identifies you.

Everything automatic is gated twice. The first act is the **TX On** latch in the header —
never remembered across launches. The second is the switch for that kind of transmission:
**AUTOREPLY** (answers SNR?, GRID?, INFO?, STATUS?, HEARING?, QUERY MSGS addressed to you,
QUERY CALL — "YES -12 (5M)", the SNR and how long ago you last heard the station asked about,
on @ALLCALL too but once per station in 15 minutes — and AGN? with your last transmission again,
and acknowledges with an ACK a MSG to you or to a group you joined, or a MSG TO: you hold for
another station, as JS8Call does; with it off, the answer, the ACK or a relay is put in the
message box for you to send instead, and nothing keys by itself; QUERY CALL and QUERY MSGS are
not answered at all with it off, as in JS8Call),
**RELAY** (passes on messages routed through your callsign and holds MSG TO: traffic),
**HB ACK** (answers heartbeats with your report, but only while **HB NET** and **AUTOREPLY** are
on as well, as in JS8Call) — those three persist in Settings, at
JS8Call's defaults (on, on, off) — and **HB**, the heartbeat schedule, which is
session-only. A chip lights only when what it switches can go out now. One that is on but
cannot says why when you point at it (TX is off, the idle watchdog holds it, or it needs
another switch), and nothing keys.
Replies about held messages read as JS8Call 3.0's: `YES MSG ID 4 +2` says two more wait after
message 4, a delivery ends `NEXT MSG ID 5` when another waits, and a heartbeat acknowledgement
carries `MSG ID 4 +2)`, as JS8Call 3.0.3 sends it.

**Heartbeats, as JS8Call 3.0 has them.** **HB NET** is JS8Call's *Enable Heartbeat
Networking*: off by default and remembered, it sends nothing by itself, and without it there
is no **HB** button and no heartbeat acknowledgement. **HB** starts the repeat (Settings sets
the interval; the first heartbeat goes one interval after you switch it on), or at interval 0
sends one heartbeat. Its menu has **Send heartbeat now** and, with an interval set, **Start**
or **Stop heartbeat timer**. Heartbeats and acknowledgements go out at Slow, Normal and Fast
only: at Turbo **HB** disappears, **HB NET** is greyed out, and a running repeat stops. As in
JS8Call, a heartbeat counts as @ALLCALL traffic: a station's heartbeat is acknowledged at most once
in 15 minutes, and a station just acknowledged, or just answered on @ALLCALL, gets neither again
for 15 minutes; the log says when a heartbeat goes unacknowledged for this. With the
**QSO pause** (on by default), a call in the **To** box pauses the heartbeat repeat until you
clear the box, holds back every acknowledgement, and stops a repeating CQ; switching **HB** on
clears the **To** box. When **HB ACK** is on but held back for one of these reasons (the speed,
the QSO pause, or a message still arriving) it shows as not armed and says why. Settings also
has JS8Call's *Allow heartbeats outside 500–1000 Hz* (your own TX offset instead of a free
spot), *Don't acknowledge stations that heartbeat more often than every 55 minutes* (such a
station gets no acknowledgement again; **HB ACK** names them, and turning the option off
forgets them) and *Do not take part in @ALLCALL* (nothing to @ALLCALL is answered or marked
as yours), all off by default.
Every automatic reply first asks in the dock, as JS8Call asks: "A transmission is queued for
autoreply … would you like to send this transmission?" **Yes** sends it in the next period; **No**
sends nothing, and so does no answer, which is No after 89 seconds. A message you hold for
someone is handed over only on Yes. Turn **Ask for confirmation before sending automatic replies**
off in Settings and a reply goes by itself in the period after the query, as JS8Call answers:
it shows in the queue meanwhile, and **Drop queue** takes it back. As in JS8Call, no reply is made while your message box holds text (you are typing, a reply
waits there, or a message of yours is going out) or while a message to you is still arriving.
After an hour with nothing typed (the idle watchdog; adjustable), heartbeats,
auto-reply and relay switch themselves off, nothing more is filed or held, and the dock says so.

**Stop TX** (header, or **Esc** anywhere in the screen) cuts the frame on the air, empties
the queue, drops a reply still asking and stops the heartbeat schedule. **Drop queue** only
empties the queue — a frame already on the air finishes. Turning **TX Off** lets the
frame in flight complete, as in the FT8 screen; what is still queued, the rest of that message
included, is dropped, not sent when TX comes back on. So is what waits while the dial is outside
your license privileges, and the screen says so.

## Compatibility

On the air: JS8Call 2.2 and later, and JS8Call-improved, at Slow, Normal, Fast and Turbo.
Not implemented: the "JS8 60" / Ultra speed (JS8Call has offered it since 3.0), and the
APRS-IS and @JS8NET internet gateways. The full protocol reference is
[JS8Call's source](https://github.com/js8call/js8call), read as the specification; see
NOTICE.
