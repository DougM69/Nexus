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
speed, which speeds to decode, heartbeat interval, the three automatic-reply switches,
the idle watchdog and your INFO/STATUS/group texts — are in
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

**The panes** (each removable from ⊞ Panels). The three lists and the Inbox are JS8Call's own;
each list's **⋯** in its title bar holds what JS8Call's View menu and right-click menus hold, and
each row has a **⋯** of its own (a right-click on the row opens it too).

- **RX** — JS8Call's RX pane: what is sent on your RX offset, and what is addressed to you or
  a group you joined wherever it is in the passband, each message on one line as its frames
  arrive ("time · offset · text"), closed by **♢** after its last frame. **……** stands where a
  frame did not come within one and a half periods. Your own transmissions print here too,
  tinted, and lines addressed to you carry a bar. Double-click a line to write to the station
  that opens it. **⋯** ▸ **Clear** empties it; **Clear all lists…** empties the three lists after
  asking.
- **Band activity** — one row per audio offset holding every frame heard there (the last ten),
  joined, with ♢ after each message. Columns: offset, age, SNR, and from **⋯ ▸ Show columns**
  time delta and speed too, as JS8Call's; **⋯ ▸ Sort by** offers JS8Call's orders; **⋯ ▸ Show
  band heartbeats and ACKs** (on) hides heartbeats and their acknowledgements. CQ rows are
  tinted green, traffic to you carries a bar, and the two highlight lists in Settings tint their
  words. Double-click a row to move RX there. A row's **⋯**: **Jump to … Hz** (both your RX and
  TX offsets, as JS8Call's one offset; not while a message is queued or going out), **Jump to …
  speed**, **Remove activity**.
- **Stations** — JS8Call's call list: `@ALLCALL` and your groups on top, then everyone heard,
  sorted by callsign (or any of JS8Call's orders from **⋯ ▸ Sort by**), a station with a
  message for you first. One mark per station: **✕** blocked, **⚑** a message for you, **★**
  hearing you (it sent something to your call), **☎** called CQ in the last five minutes,
  **⚟** heard only through a relay. Columns as JS8Call's: callsign, age, SNR, offset, worked
  before ✓, logged name and comment, and from **⋯ ▸ Show columns** time delta, speed, grid,
  distance and azimuth. Click a callsign to address it; the five quick buttons send **SNR? ·
  GRID? · INFO? · HEARING? · QUERY MSGS** to that station, which answers by itself if its
  auto-reply is on; **HB** and **✉ n** chips show a heartbeat and messages you hold for it. A
  row's **⋯**: jump to its offset or speed, **Show message inbox**, **Pin to the top**, **Add new
  station or group…** (a call, or a `@GROUP` to join), **Remove this station** (until it is
  heard again) or **Remove this group** (leaves it), **Block this station** (Settings' RX block
  list).
- **Inbox** — JS8Call's message panel: messages addressed to you, and **MSG TO:** messages you
  are holding for someone else until they collect them (store and forward). Newest first;
  click a heading to sort by it. **Messages:** shows one station's. Clicking a message reads
  it; tick several and **Delete** them together (or press Delete). **Reply** puts
  `<path> MSG [MESSAGE]` in the compose box, the path the message came by, with `[MESSAGE]`
  picked for you to type over; nothing is sent until you send it. A new message to you puts a
  notice up for five minutes, as JS8Call's box stays, with **Show** to open it.
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
variant picker and **CQ**; **HB**; the **AUTOREPLY · RELAY · HB ACK** chips; an automatic
reply's question, **Yes** or **No**; the queue with **Drop queue**; and the
idle-watchdog readout.

## Receiving

All four speeds decode at once, exactly as JS8Call's multi-decode does: a Slow station at
−26 dB and a Turbo station rattling off a paragraph on the same band both print. Untick
speeds in Settings if a small machine struggles. A long message arrives one frame per
period and is stitched together by audio offset — you see the frames arrive in Band
activity (and in RX when they are on your offset or for you), and the finished message in
Inbox if it was for you. Settings ▸ Digital ▸ JS8 holds JS8Call's lists and marks: the callsigns
to hide in the RX pane, two highlight lists, the ♢ and …… marks, and a sound for each of a CQ,
a heartbeat, an ACK for you, a message to you, a message in your Inbox, a new callsign and one
you have worked (all off, as in JS8Call).

## Transmitting

JS8 keys **one frame per period, every period**, with no even/odd parity — a 60-character
message at Normal is five frames, 75 seconds. **Send** queues the frames; the next period
boundary sends the first. Plain text (no addressee) goes out prefixed with your callsign,
so every frame identifies you.

Everything automatic is gated twice. The first act is the **TX On** latch in the header —
never remembered across launches. The second is the switch for that kind of transmission:
**AUTOREPLY** (answers SNR?, GRID?, INFO?, STATUS?, HEARING?, QUERY MSGS addressed to you,
and acknowledges with an ACK a MSG to you or to a group you joined, or a MSG TO: you hold for
another station, as JS8Call does; with it off, the answer, the ACK or a relay is put in the
message box for you to send instead, and nothing keys by itself),
**RELAY** (passes on messages routed through your callsign and holds MSG TO: traffic),
**HB ACK** (answers heartbeats with your report, but only while **HB** and **AUTOREPLY** are on
as well, as in JS8Call) — those three persist in Settings, at
JS8Call's defaults (on, on, off) — and **HB**, the heartbeat schedule, which is
session-only. A chip lights only when what it switches can go out now. One that is on but
cannot says why when you point at it (TX is off, the idle watchdog holds it, or it needs
another switch), and nothing keys.
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
