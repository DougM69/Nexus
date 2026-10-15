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

**The header.** A **JS8** badge, then the five speed chips — **Slow · Normal · Fast ·
JS8 40 · JS8 60** (30 / 15 / 10 / 6 / 4 second periods), named as JS8Call 3.0 names them:
JS8 40 is the speed called Turbo before 3.0, and the chip's tooltip says so; JS8 60 is 3.0's
newest speed (its tooltip calls it Ultra, its name in JS8Call's code). The lit one is the speed
*you* transmit at and the period the TX clock follows. Beside them an **RX ▼ n/5** plate says
how many of the five speeds the receiver is decoding right now (all five by default). Then the band
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
  heard again) or **Remove this group** (leaves it; on `@ALLCALL` it turns on Settings' *Do not
  participate in the @ALLCALL group* and the row goes, and adding `@ALLCALL` turns it off and
  brings the row back, as in JS8Call), **Block this station** (Settings' RX block list).
  **Store…** at the end of a row leaves a message at your station for that station, as
  JS8Call's Store Message does: it waits in the Inbox, from your call, and goes out only when
  the station asks for it (QUERY MSGS, then QUERY MSG n). Storing it sends nothing.
- **Inbox** — JS8Call's message panel: messages addressed to you, and **MSG TO:** messages you
  are holding for someone else until they collect them (store and forward). Newest first;
  click a heading to sort by it. **Messages:** shows one station's. Clicking a message reads
  it; tick several and **Delete** them together (or press Delete). **Reply** puts
  `<path> MSG [MESSAGE]` in the compose box, the path the message came by, with `[MESSAGE]`
  picked for you to type over; nothing is sent until you send it, and while a message of yours
  is going out it puts nothing in, as JS8Call adds no text then. A new message to you puts a
  notice up for five minutes, as JS8Call's box stays, with **Show** to open it. A message held
  for one station stays until you delete it, as in JS8Call. One left for a group
  (`MSG TO:@GROUP`, a group you joined) is offered for 48 hours to every station that sends
  QUERY MSGS to the group, and handed to each of them once. The inbox keeps at most 100 messages
  (and 64 KB) of yours and as many held for others; when it is full, a message already handed
  over goes first, then the oldest, and a note above the messages says which message went and
  why until you press **Clear notes**. A held message is handed over in one transmission, which
  holds at most 19 frames at Slow, 39 at Normal, 59 at Fast and 99 at Turbo; one that needs more
  is not offered or handed over until you change to a speed it fits, and its row says so and
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
45 s") and **Send**; a command palette with JS8Call's 32 directed commands; JS8Call's
**REPLY · SNR · INFO · STATUS** buttons and **Saved** menu (see [Composing](#composing));
**CQ**; **HB NET** and, while it is on, **HB** with its menu (**▾**, or
right-click **HB**); the **AUTOREPLY · RELAY · HB ACK** chips; an automatic
reply's question, **Yes** or **No**; the queue with **Drop queue**; and the
idle-watchdog readout.

## Receiving

Every speed decodes at once, exactly as JS8Call 3.0's multi-decode does: a Slow station at
−26 dB and a JS8 60 station rattling off a paragraph on the same band both print. Untick
speeds in Settings if a small machine struggles. Settings saved before JS8 60 arrived keep the
four speeds they had; tick **JS8 60** under *Decode these speeds* to add it. A long message
arrives one frame per period and is stitched together by audio offset — you see the frames
arrive in Band activity (and in RX when they are on your offset or for you), and the finished
message in Inbox if it was for you. Settings ▸ Digital ▸ JS8 holds JS8Call's lists and marks:
the callsigns to hide in the RX pane, two highlight lists, the ♢ and …… marks, and a sound for
each of a CQ, a heartbeat, an ACK for you, a message to you, a message in your Inbox, a new
callsign and one you have worked (all off, as in JS8Call).

**JS8 60** is JS8Call 3.0's newest speed, and JS8Call calls it experimental: 4-second
periods, a signal 250 Hz wide, each frame about 2.6 s on the air, so the band carries a
paragraph in a few periods — when signals are strong. It needs the most signal of the five
(JS8Call puts its floor at −18 dB on its own scale, against Normal's −24). Pick it only
when the station you are working copies you well, and do not expect JS8Call 2.x stations to
see it: only JS8Call 3.0 and later decode it.

## Composing

The compose box works as JS8Call 3.0's does. Its switches and texts are in
[Settings ▸ Digital ▸ JS8](settings-reference.md#js8), under **Composing**.

- **Pills.** As you type, the parts of a message JS8Call recognises are drawn as coloured
  pills: who it goes to (a call, a relay route such as `W1AW>K1ABC`, or a `@GROUP`), who it is
  from, and the command (`SNR?`, `MSG`, `QUERY CALL K1ABC?` and the rest). Point at a pill to
  read what it does. A command typed with no address goes to the call in the **To** box, and
  the pill says so.
- **Cheaper words.** A word the JS8 dictionary sends in pieces is underlined in red.
  Right-click it for up to five words that take less airtime, cheapest first, and pick one to
  put it in place.
- **One-click messages.** **REPLY** sends your reply message (`HW CPY?` unless you change it)
  to the call in the **To** box. **SNR** sends that station the report you hear from it, once
  it has been heard. **INFO** and **STATUS** send yours, to the **To** box call or to everyone.
  **CQ** sends your CQ message (`CQ CQ CQ` and your square unless you change it), and clears
  the **To** box as JS8Call does. A button that sends leaves what you were typing in the box.
- **Saved messages.** The **Saved** menu lists the messages you keep (`TNX 73 GL` to start
  with). Pick one to send it, to the **To** box call or to everyone. **Save current message**
  adds what is in the compose box, **Edit saved messages…** opens Settings, and **Restore
  previous message** puts the last message you sent back in the box.
- **Macros.** A saved message, the CQ and reply messages, INFO and STATUS may carry JS8Call's
  macros, replaced when the message is used: `<MYCALL>`, `<MYGRID4>`, `<MYGRID12>`, `<MYINFO>`,
  `<MYCQ>`, `<MYHB>`, `<MYREPLY>`, `<MYSTATUS>`, `<MYVERSION>`, `<MYIDLE>`, and for the call in
  the **To** box once it has been heard, `<CALL>`, `<SNR>` and `<TDELTA>`. A macro typed into
  the compose box is replaced when you press **Send**.
- **Fill only.** Turn off **Send CQ, REPLY, SNR, INFO, STATUS, saved messages and a station's
  asks at once** (JS8Call's "Immediately transmit…") and those buttons, and the quick asks on a
  station row, put their message in the compose box instead; **Send** sends it. The repeating
  CQ always sends.

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
only: at JS8 40 and JS8 60 **HB** disappears, **HB NET** is greyed out, and a running repeat stops. As in
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

**Typeahead.** After **Send** the message stays in the message box while it goes out, as
JS8Call's does: the box shows it as it goes on the air (with your callsign and the addressee in
front), and as each frame is sent its text moves out of the box to the line beside it. Keep typing
and the frames still to go carry what you typed; the change is taken just before the next frame.
Text typed after the last frame has gone goes straight after it, as a message of its own. Delete
everything still to go and the message ends after the frame on the air. **Enter** and **Send** do
not queue a new message while yours is going out. A MSG or relay message cannot be changed while it
goes out (the box is read-only), as in JS8Call. While it goes out, **REPLY**, **SNR**, **INFO**,
**STATUS** and **Saved** are greyed out and a button that fills the box adds nothing, as JS8Call
adds no text then. A macro typed into the box meanwhile is replaced as **Send** replaces it, except
`<CALL>`, `<SNR>` and `<TDELTA>`, which stay as typed. The box empties when the message has gone,
or when you press **Stop TX**.

**Between frames.** The radio drops PTT after each frame and keys again for the next, as JS8Call
does by default: at Normal it is unkeyed for about 1.6 seconds between two frames (Slow about 4,
Fast and JS8 40 about 1.7, JS8 60 about 1.1). Turn on **Hold PTT between frames** in Settings ▸
Digital ▸ JS8 and it stays keyed from the end of one frame of a message to the start of the next
instead. It still drops after the message's last frame, never holds from one message into the
next, and drops at once on **Stop TX** or **TX Off**.

**Moving the TX offset while sending.** While a message is waiting to go or going out, the TX
offset stays where it is, as in JS8Call: a right-click on the waterfall is refused and the screen
says why. Turn on **Allow TX offset changes while sending** and the move is taken; the frames still
to go are sent at the new offset (the frame on the air finishes where it started). Moving RX is
never held.

## Compatibility

On the air: JS8Call 2.2 and later, and JS8Call-improved, at Slow, Normal, Fast and JS8 40
(Turbo); JS8Call 3.0 and later at JS8 60 as well, a speed JS8Call 2.x does not decode.
Not implemented: the APRS-IS and @JS8NET internet gateways. The full protocol reference is
[JS8Call's source](https://github.com/js8call/js8call), read as the specification; see
NOTICE.
