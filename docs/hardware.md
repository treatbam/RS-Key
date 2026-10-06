<!-- SPDX-License-Identifier: AGPL-3.0-only -->
<!-- Copyright (C) 2026 RS-Key contributors -->

# Hardware

What RS-Key runs on, and the build knobs you need for a board other than the
reference one. The full knob reference is in [build.md](build.md). This page is
the short version.

## Supported boards

<table>
<tr>
<td align="center"><img src="images/board-one.jpg" width="160" alt="Waveshare RP2350-One — a blue USB-A stick board"></td>
<td align="center"><img src="images/board-zero.jpg" width="160" alt="Waveshare RP2350-Zero — a small blue USB-C stick board"></td>
<td align="center"><img src="images/board-tenstar.jpg" width="160" alt="TenStar RP2350-USB — a black USB-A stick board"></td>
<td align="center"><img src="images/board-display.jpg" width="160" alt="Waveshare RP2350-Touch-LCD-2.8 — a board with a 2.8-inch touch screen"></td>
</tr>
<tr>
<td align="center"><b>RP2350-One</b><br>reference · USB-A<br>WS2812 on GPIO16</td>
<td align="center"><b>RP2350-Zero</b><br>mini stick · USB-C<br>(+ ILI9341 touch panel, <a href="#ili9341--xpt2046-on-an-rp2350-zero">below</a>)</td>
<td align="center"><b>TenStar RP2350-USB</b><br>USB-A stick<br>WS2812 on GP22</td>
<td align="center"><b>RP2350-Touch-LCD-2.8</b><br>trusted display · 2.8″ LCD</td>
</tr>
</table>

<sub>Board photos: Waveshare (RP2350-One / Zero / Touch-LCD-2.8).</sub>

Any RP2350 board with a USB connector should work. Development and on-device
testing happen on the **Waveshare RP2350-One**, where the WS2812 status LED on
GPIO16 works out of the box. Boards without an addressable LED run fine. The
indicator is optional and the firmware just runs dark. A board whose LED sits
behind a power gate — the **Seeed XIAO RP2350** powers its WS2812 through GP23 —
lights up once that enable pin is set with `LED_POWER_PIN` (below).

The RP2350's dual Cortex-M33, 520 KB SRAM, hardware TRNG, OTP fuses, and glitch
detectors do the work. There is **no secure element** and no debugger
requirement: the firmware flashes over USB BOOTSEL, so a bare board and a USB
cable are enough.

## Defaults and the knobs to change them

The default build targets a 4 MB flash chip with the LED on GPIO16, uses BOOTSEL
for user presence, and assumes a standard 12 MHz crystal. For a different board,
three compile-time knobs usually cover it:

| Knob | Default | When to change it |
|---|---|---|
| `FLASH_SIZE` | `4M` | A board with a different QSPI flash chip (e.g. `8M`). `build.rs` regenerates `memory.x` from it. Must be ≤ 16 MB and leave ≥ 1 MB for code after the KV store; a **2 MB** board also needs a smaller `KVMAIN` (below). |
| `KVMAIN` | `1408K` | A **2 MB** board (Seeed XIAO RP2350, Waveshare RP2350-Zero-CM): the default 1408K KV main partition leaves too little for the ~900K image. Shrink it — `FLASH_SIZE=2M KVMAIN=896K` — to fit. A fully provisioned key uses only a few hundred KB. See [build.md](build.md). |
| `LED_PIN` | `16` | A board that uses GPIO16 for something else, or wires its addressable LED elsewhere (RP2350A: GPIO `0..=29`). |
| `LED_POWER_PIN` | `none` | A board whose LED sits behind a power gate that must be driven **high** to light it (e.g. the Seeed XIAO RP2350's WS2812 on GP23). Set the enable GPIO; it must differ from `LED_PIN` and any GPIO `PRESENCE_PIN`. |
| `USR_LED_PIN` | `none` | A board with a *nuisance* onboard user LED that lights by default (the Seeed XIAO RP2350's active-low USR LED on GP25). Set the GPIO to park it off at boot; flip `USR_LED_ACTIVE_HIGH=1` for an active-high LED. Must differ from `LED_PIN`, `LED_POWER_PIN`, and any GPIO `PRESENCE_PIN`. |
| `PRESENCE_PIN` | `bootsel` | A board with a dedicated user-presence button on a GPIO. Set a pin number (`0..=29`); active-low with a pull-up by default (e.g. `0` for GPIO0-to-GND). |
| `PRESENCE_ACTIVE_HIGH` | `0` | A presence button/sensor that reads **high** when pressed (a capacitive touch sensor, or a button to VCC). `1` flips the GPIO to pull-down + active-high. Only with a GPIO `PRESENCE_PIN`. |
| `LED_KIND` | `ws2812` | `ws2812` (addressable RGB, default), `gpio` (plain on/off), `pimoroni` (3-pin PWM RGB), or `none` (no indicator). See [build.md](build.md). |
| `LED_ORDER` | `rgb` | A `ws2812` board whose red and green come out swapped (blue fine): set `grb` (the WS2812B standard). The Waveshare RP2350-One is `rgb`; most other parts are `grb`. |
| `MAX_LEDS` | `8` | A board with **more than 8** daisy-chained addressable LEDs. The buffer ceiling; the actual connected count is set at runtime ([guides/led.md](guides/led.md)). |

```sh
# example: an 8 MB board with a plain LED on GPIO25
env FLASH_SIZE=8M LED_KIND=gpio LED_PIN=25 cargo build --release -p firmware

# example: a 16 MB TenStar RP2350-USB — WS2812 on GP22, standard GRB order
env FLASH_SIZE=16M LED_PIN=22 LED_ORDER=grb cargo build --release -p firmware

# example: a Seeed XIAO RP2350 — WS2812 on GP22, GRB, power-gated by GP23 (driven high),
# its active-low USR LED on GP25 parked off, and its 2 MB flash (smaller KVMAIN)
env FLASH_SIZE=2M KVMAIN=896K LED_PIN=22 LED_ORDER=grb LED_POWER_PIN=23 USR_LED_PIN=25 cargo build --release -p firmware

# example: WS2812 on GP22 and a button-to-GND on GP0 (active-low)
env LED_PIN=22 PRESENCE_PIN=0 cargo build --release -p firmware

# example: an active-high capacitive touch sensor on GP0
env PRESENCE_PIN=0 PRESENCE_ACTIVE_HIGH=1 cargo build --release -p firmware
```

The four LED knobs (`LED_PIN` / `LED_KIND` / `LED_ORDER` / `MAX_LEDS`) set only the
*boot defaults*. A non-`none` build compiles all three backends, so the pin,
driver, wire order, and buffer ceiling are also changeable at **runtime** (no
reflash) with `rsk hw` or PicoForge, which write them to the device's `phy`
record
([guides/led.md](guides/led.md)). The build knobs still matter for picking a
lean `none` build and for the out-of-the-box default.

Most RP2350A boards work with at most a one-line change. Everything else
(USB descriptors, applets, flash layout) is board-independent.

## ILI9341 + XPT2046 on an RP2350-Zero

**Experimental.** The [trusted-display](guides/display.md) flavor also runs on
hand-wired parts: a **Waveshare RP2350-Zero** (4 MB) and a common 2.4″ 240×320
**ILI9341** SPI module with an **XPT2046** resistive touch controller (the red-PCB
"2.4 inch SPI TFT with touch" kind). The panel is the same resolution as the
RP2350-Touch-LCD-2.8, so every screen is reused unchanged. It is opt-in: the
default image and the 2.8″ flavor are unaffected.

```sh
env BOARD=waveshare-zero-ili9341 cargo build --release -p firmware --features display
scripts/pt.sh target/thumbv8m.main-none-eabihf/release/firmware firmware-pt.elf
picotool uf2 convert firmware-pt.elf -t elf firmware.uf2
```

`BOARD=waveshare-zero-ili9341` is shorthand for `DISPLAY_PANEL=ili9341` plus the
pins below; `DISPLAY_PANEL=ili9341` alone gets the same defaults. (The knob is not
named `DISPLAY`: that is the X11 variable every Linux desktop session exports.)

| Signal | RP2350-Zero | Module pin | Notes |
|---|---|---|---|
| SPI0 SCK | GP2 | `SCK` + `T_CLK` | shared bus, fixed |
| SPI0 MOSI | GP3 | `SDI (MOSI)` + `T_DIN` | shared bus, fixed |
| SPI0 MISO | GP4 | `T_DO` (+ `SDO (MISO)`) | shared bus, fixed; the panel is never read |
| LCD CS | GP5 | `CS` | `cs` |
| Touch CS | GP6 | `T_CS` | `touch_cs` |
| LCD DC | GP7 | `DC` | `dc` |
| LCD reset | GP8 | `RESET` | `rst` |
| Touch IRQ | GP9 | `T_IRQ` | `touch_irq`, active low, internal pull-up |
| Backlight | GP10 | `LED` | `bl_pin`; PWM slice 5 A, active high (direct or through an N-FET) |
| Status LED | GP16 | — | the Zero's onboard WS2812, kept as the status indicator |
| Power | 3V3 / GND | `VCC` / `GND` | |

The knob names in the last column are `[display]` keys in
[`firmware/boards/waveshare-zero-ili9341.toml`](https://github.com/TheMaxMur/RS-Key/blob/main/firmware/boards/waveshare-zero-ili9341.toml);
copy the file to rewire. The SPI0 pins are fixed. The backlight must be a channel-A
GPIO among 6/8/10/12/14 so it gets a PWM slice of its own, and every pin is checked
against the others, the LED pins and the wake button at compile time. User presence
is the on-screen Approve, as on the 2.8″ board — BOOTSEL is not read on a display
build. There is no wake button by default — a tap wakes the panel from display
sleep — but `WAKE_PIN=<gpio>` can add one (active low unless `WAKE_ACTIVE_HIGH=1`).

What differs from the 2.8″ board:

- **Hardware SPI, shared.** Panel and touch sit on one SPI0 bus. The panel runs
  at 37.5 MHz (the 40 MHz request rounded to `clk_peri`/4) with DMA; each touch
  read re-clocks the bus to 2 MHz (the XPT2046 tops out at 2.5 MHz) and restores
  it. A full-screen redraw takes about twice as long as on the 2.8″ board's
  80 MHz PIO link. In exchange the chip stays at the stock 150 MHz: this flavor
  sets no overclock.
- **Panel orientation and colour.** Most ILI9341 modules want BGR order, no
  inversion and the MADCTL column mirror for an upright portrait image —
  `color_order = "bgr"`, `invert_colors = false`, `mirror_x = true`. A module that
  comes up mirrored or with red and blue swapped needs those flipped.
- **Resistive touch.** A read happens only while `T_IRQ` is low. It takes a
  pressure pair and five samples per axis, uses the median, and drops a press that
  is too light or a burst that spreads more than a few pixels. A contact counts
  once two reads in a row agree. Press with a fingernail or stylus. A resting palm
  or a light brush is not a tap.
- **Calibration is build-time.** `touch_x_min`/`touch_x_max`/`touch_y_min`/
  `touch_y_max` give the raw 12-bit reading at the panel's left/right/top/bottom
  edge; a `min` above its `max` flips the axis, and `touch_swap_xy = true` swaps
  the axes. The defaults (`3800`/`300`/`300`/`3800`, no swap) fit the common
  module under the default orientation. If taps land mirrored, swap that axis's
  `min` and `max`; if horizontal taps move vertically, set `touch_swap_xy`. There
  is **no on-device calibration screen and no raw-value readout yet**. To measure
  a module, read its corners with any XPT2046 test sketch and put the numbers in
  the board file.

> ⚠️ **Hand-wired means the wires are part of the trusted display.** The ceremony
> rests on "a tap on a screen showing the true relying party". With the bus on
> loose jumpers, anyone who can reach them can inject a touch (driving `T_IRQ` and
> `MISO` fakes an Approve), repaint the panel over `MOSI`, or read PIN-pad taps off
> `MISO`, since the touch coordinates cross the bus in the clear. The 2.8″ board
> keeps those traces on its PCB. Physical attacks are outside the
> [threat model](threat-model.md), but this build makes them cheaper: keep it in a
> closed case.

## Enclosures

A bare board works fine, but a printed case makes it pocketable. Four community
designs fit the boards above:

- **[Waveshare RP2040-One / RP2350-One case](https://www.printables.com/model/1129764-waveshare-rp2040-one-and-rp2350-one-case)**
  by Patrick van der Leer. Sized for the reference board.
- **[RP2350 USB case](https://www.printables.com/model/1248338-rp2350-usb-case)**
  by Vladimir Varzaru (a remix of Patrick's design). A slimmer USB-stick form.
- **[Pico 2 Zero FIDO2 case (RP2350)](https://makerworld.com/en/models/2655625-pico-2-zero-fido2-case-rp2350)**
  on MakerWorld. Fits the RP2350-Zero.
- **[Waveshare RP2350 2.8-inch case](https://makerworld.com/en/models/3204749-waveshare-rp2350-2-8inch-case)**
  by [@cofob](https://cofob.dev/). Designed for RS-Key and recommended for the
  RP2350-Touch-LCD-2.8. PETG is the recommended print material.
- **[Waveshare RP2350 2.8-inch case](https://www.thingiverse.com/thing:7404751)**
  by Mono. Designed for RS-Key, reliable USB and button performance as well as
  sturdyness. PETG material recommended.

The two Printables designs as well as the Thingiverse design are licensed
**CC BY-SA 4.0**: print, sell, and remix them freely, as long as you credit the
authors and keep any derivative under the same license. Check each MakerWorld
page for the design license before you reuse the design. All are third-party
designs, linked for convenience, not part of this project.

## What the hardware does not give you

The OTP fuses and secure boot ([production.md](production.md)) are real
hardening, but the RP2350 is a general-purpose microcontroller, not a certified
secure element. Physical attacks are out of scope: decapping, microprobing,
fault injection beyond the on-chip glitch detectors, power/EM side channels.
See the [threat model](threat-model.md) and [limitations](limitations.md).
