# TCI: connect to an SDR program's TCI server (Beta)

> **Field status: not verified on hardware.** Nexus's TCI connection is tested
> against a simulated TCI server only. Nobody has run it here against a real
> ExpertSDR3, Thetis or piHPSDR yet. The programs' menus named below are taken
> from their own documentation; where your version differs, go by what your
> program shows.

TCI (Transceiver Control Interface) is Expert Electronics' protocol for SDR
programs. The program runs a small server, and other programs connect to it
over one network connection. ExpertSDR3 (SunSDR2, MB1, Colibri), Thetis from
version 2.10.3.14 (ANAN, Hermes Lite 2) and piHPSDR serve it, and WSJT-X, JTDX
and many loggers connect to it as clients. Nexus can be one of those clients.

With TCI, Nexus talks to the SDR program directly: no rigctld, no Hamlib rig
model and no virtual COM port.

---

## What works today

The TCI connection is **Beta**, and it arrives in parts:

| Part | Today |
|---|---|
| The dial and the mode, read and set through the program's TCI server | Works |
| **Find TCI server**, in Settings and in the setup wizard | Works |
| Receive audio over TCI | Not yet: the audio still comes from the sound card under **Audio**, so keep the program's virtual audio cable for now |
| Transmitting over TCI | Not yet: **Nexus does not transmit on this connection** |
| The program's panadapter in Nexus's scope | Not yet |
| The stations you need, shown on the program's panadapter | Not yet |

The switches for audio, the panadapter and spots are in Settings already and
are saved with the radio. They take effect when those parts arrive.

To transmit with an SDR program today, use its CAT server instead, as
[Hermes Lite 2 and SDR Program Setup](sdr-programs.md) describes.

---

## Turn the TCI server on in the program

- **Thetis:** **Setup ▸ Serial/Network/Midi CAT ▸ Network**, the **TCI
  Server** box beside the TCP/IP CAT Server one. As Thetis comes it is
  `127.0.0.1:50001`. Turn it on there.
- **piHPSDR:** **Menu ▸ CatTci**, the **TCI** row. Its port is 40001 unless
  you change it. Tick **Enable** on that row.
- **ExpertSDR3:** turn its TCI server on in the program's settings, and note
  the port it shows. Owners report 50001.

The program has to be running, with its TCI server on, before Nexus can
connect to it.

---

## Connect Nexus

1. Open **Settings ▸ Radio ▸ Rig & CAT**.
2. Set **Connection** to **TCI (SDR program) — Beta**. Rig Model, Serial Port
   and Baud go away: the SDR program drives the radio, so there is nothing to
   pick.
3. In **SDR program address**, leave `127.0.0.1` when the program runs on this
   computer. For another computer on your network, type its address, such as
   `192.168.1.30`.
4. Press **Find TCI server**. Nexus looks at that address: the port in **TCI
   port** first, then 50001 and 40001. It says what answered, such as
   "Found a TCI server at 127.0.0.1:50001: ExpertSDR3 2.0, SunSDR2DX. It has
   2 transceivers." A server found on another port fills that port in.
5. **Save.**

Find TCI server only reads what the server says about itself (the program and
TCI version it announces, the radio it drives, how many transceivers it has)
and then closes the connection. It never transmits and never changes anything
on the radio.

### The fields

| Field | What it is | As it comes |
|---|---|---|
| **SDR program address** | The computer the SDR program runs on. | `127.0.0.1` (this computer) |
| **TCI port** | The port in the program's TCI settings. | 50001 |
| **Transceiver** | Which of the program's transceivers this radio is, counted from 1. | 1 |
| **SDR program** | **Automatic** learns the program from what it does. Name ExpertSDR3, Thetis or piHPSDR only if Nexus reads yours wrongly; naming one fills in the port it comes set to. | Automatic |
| **Audio over TCI** | Receive audio from the program, and Nexus's transmit audio to it, over TCI instead of a virtual audio cable. Not yet in effect. | On |
| **SDR panadapter in the scope** | The program's own spectrum in Nexus's scope and waterfall. Not yet in effect. | Off |
| **Spots on the SDR panadapter** | The stations you still need, marked on the program's panadapter, 100 at most. Clicking one tunes Nexus to it; it never transmits. Not yet in effect. | Off |

Every field is per radio: each radio card keeps its own.

### In the setup wizard

The wizard's **How does the radio connect?** step looks for a TCI server on
this computer (ports 50001 and 40001) when it opens, and offers one it finds as
a row to pick. It also has an **SDR program (TCI)** choice, with the address,
the port and **Find TCI server**.

---

## When Find TCI server finds nothing

- **"No TCI server answered."** The program is not running, its TCI server is
  off, or it listens on another port. Turn the server on, type its port in
  **TCI port**, and press Find again.
- **"Something answered on port …, but it is not a TCI server."** Another
  program holds that port. Check the port in the program's TCI settings.
- **"… answered but did not finish the TCI handshake."** Something answered as
  a WebSocket server and stopped there, such as a program still starting up.
  Wait a moment and try again.
- **"Port … is Nexus's own TCI server, not an SDR program."** Nexus can serve
  TCI to other programs too ([Share by TCI](tci-server.md)), and that is what
  answered on that port. Point the connection at the SDR program's port
  instead.
- **"This computer cannot find …"** The address is not one this computer can
  look up. Check **SDR program address**.

---

## Your own network only

TCI has no password and no encryption. Use it on this computer or on your own
network; from anywhere else, use a VPN, and never forward a TCI port to the
internet.

---

## Related guides

- [Hermes Lite 2 and SDR Program Setup](sdr-programs.md): the CAT server route
  for Thetis and piHPSDR, which carries transmit and audio today.
- [Share by TCI](tci-server.md): the other direction, Nexus as the TCI server
  for programs such as WSJT-X on this computer.
- [Rig Setup Guides](index.md): the connection types, and Test CAT.
- [Settings reference ▸ Rig & CAT](../guide/settings-reference.md#rig--cat):
  every field in that section.
