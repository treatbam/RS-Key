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
- **Calibration is build-time seeded and runtime-calibratable on-device.**
  `touch_x_min`/`touch_x_max`/`touch_y_min`/`touch_y_max` give the boot-default raw
  12-bit reading at the panel's left/right/top/bottom edge; a `min` above its `max`
  flips the axis, and `touch_swap_xy = true` swaps the axes. The defaults
  (`3800`/`300`/`300`/`3800`, no swap) fit the common module under the default
  orientation. On-device, open **Settings → Display → Calibrate touch** to run a
  guided 3-point crosshair tap + center verification target; a verified calibration
  is persisted to `EF_TOUCH_CAL` (`0xE031`) across reboots (and automatically
  reverts if timed out or missed so a bad tap can never lock out the touch screen).

> ⚠️ **Hand-wired means the wires are part of the trusted display.** The ceremony
> rests on "a tap on a screen showing the true relying party". With the bus on
> loose jumpers, anyone who can reach them can inject a touch (driving `T_IRQ` and
> `MISO` fakes an Approve), repaint the panel over `MOSI`, or read PIN-pad taps off
> `MISO`, since the touch coordinates cross the bus in the clear. The 2.8″ board
> keeps those traces on its PCB. Physical attacks are outside the
> [threat model](threat-model.md), but this build makes them cheaper: keep it in a
> closed case.

### Stacked: the Zero soldered straight onto the module's header

**Early hardware test: flashes, draws and takes taps** (with the touch Y axis
flipped as below); not yet exercised through a full ceremony. Same parts, no loose bus wires: the Zero
sits on the module's 14-pin header so its GP0..GP8 edge lands on `CS`..`T_CS`, and
five short jumpers do the rest. SPI0 can't take that pin order (its clock pins
come right before its data-out pins; the header has MOSI before SCK), so this
preset drives the panel over the same TX-only PIO link as the 2.8″ board — on
PIO1, since the WS2812 keeps PIO0 — and bit-bangs the XPT2046 on its own pins.

```sh
env BOARD=waveshare-zero-ili9341-stacked cargo build --release -p firmware --features display
```

| Signal | RP2350-Zero | Module pin | How | Notes |
|---|---|---|---|---|
| Power | 3V3 / GND | `VCC` / `GND` | **jumper ×2** | the header's first two pins sit past the Zero's edge |
| LCD CS | GP0 | `CS` | header | `cs` |
| LCD reset | GP1 | `RESET` | header | `rst` |
| LCD DC | GP2 | `DC` | header | `dc` |
| Panel data | GP3 | `SDI (MOSI)` | header | `pio_data` |
| Panel clock | GP4 | `SCK` | header | `pio_clk` |
| Backlight | GP5 | `LED` | header | `bl_pin`; PWM slice 2 **B**, active high (the module's Q1 switches it) |
| — | GP6 | `SDO (MISO)` | header | left unclaimed: the panel is never read |
| Touch clock | GP7 | `T_CLK` | header | `touch_clk`, bit-banged at ≤2 MHz |
| Touch CS | GP8 | `T_CS` | header | `touch_cs` |
| Touch data in | GP9 | `T_DIN` | **jumper** | `touch_din` |
| Touch data out | GP10 | `T_DO` | **jumper** | `touch_dout` |
| Touch IRQ | GP11 | `T_IRQ` | **jumper** | `touch_irq`, active low, internal pull-up |
| Status LED | GP16 | — | onboard | the Zero's WS2812, kept as the status indicator |

The keys are in
[`firmware/boards/waveshare-zero-ili9341-stacked.toml`](https://github.com/TheMaxMur/RS-Key/blob/main/firmware/boards/waveshare-zero-ili9341-stacked.toml)
(`bus = "pio"` is what selects this transport; `DISPLAY_BUS=pio` does the same from
the environment). The orientation and colour keys are the hand-wired file's, so the notes above
apply unchanged. The touch window is not: stacked, the module's Y reading runs the
other way, so the preset flips it (`touch_x_min`/`touch_x_max` = `3800`/`300`,
`touch_y_min`/`touch_y_max` = `3800`/`300`, no swap), measured on hardware.

What differs from the hand-wired build:

- **Faster panel, overclocked chip.** The PIO link spends two instructions per bit
  with an integer divider, so this preset runs `clk_sys` at 160 MHz — the 2.8″
  board's clock — and the panel at 160 / (2 × 2) = **40 MHz**, DMA-fed, against
  the SPI0 build's 37.5 MHz. At the stock 150 MHz the only integer choices would be
  37.5 or 75 MHz. The 160 MHz trade is the one
  [limitations.md](limitations.md) records for the 2.8″ board. `spi_freq_hz =
  80_000_000` (divider 1) also builds; the short stacked traces may take it, but it
  is well past the ILI9341's rated write clock and untested.
- **Touch on its own pins.** The XPT2046 can't share a write-only link, so it gets
  a software SPI mode-0 transfer at ≤2 MHz (about 0.2 ms per read). The sampling,
  median, pressure check, debounce and calibration are the same code as the SPI0
  build.
- **Pin rules.** The PIO clock/data pins are checked against each other and every
  other display, LED and wake pin at compile time, like the control lines. GP5's
  PWM slice 2 is shared with the pimoroni LED backend's blue (GPIO20), so
  `LED_KIND=pimoroni` is refused at build time and a phy record asking for it keeps
  the WS2812.
- The stacked header removes the loose bus wires but not the jumpers: `T_DIN`,
  `T_DO` and `T_IRQ` still run on wires, so the case warning above still holds.

#### Optional haptic feedback (`HAPTIC_KIND`)

A haptic motor can be wired to the stacked board (or any board) to give tactile
cues on three events:

1. **Touch pulse:** a short sharp pulse (~18–20 ms) on the rising edge of each
   registered touch contact.
2. **PIN prompt:** a dull double-pulse cue (two soft, low-intensity pulses) when
   the PIN pad opens for entry.
3. **PIN rejection:** one long buzz (~450 ms) when a PIN is rejected (on-screen
   PIN pad retry/lockout, CTAP2 clientPIN / built-in UV rejection, or CCID pinpad
   `VERIFY` failure). Triggered only after the verification result is decided,
   never inside constant-time comparison paths.

Haptics run in a dedicated non-blocking Embassy task; events coalesce (`PinError`
overrides `PinPrompt`, which overrides `Touch`) and never delay touch sampling or
crypto work. By default `HAPTIC_KIND=none` compiles zero haptic code and claims
no pins.

Two hardware backends are supported at compile time via `[haptic]` in the board
TOML or environment overrides:

- **Direct PWM ERM coin motor (`HAPTIC_KIND=pwm`):** drives an eccentric rotating
  mass (ERM) pancake motor from a GPIO PWM output (`HAPTIC_PIN`, default **GP26**,
  PWM slice 5 channel A at ~19.5 kHz carrier; duty cycle sets intensity: 100% for
  touch and error buzz, ~40% for the soft double-pulse prompt). Because a bare
  RP2350 GPIO cannot source the 50–100 mA an ERM motor draws, switch the motor on
  the low side through an **N-channel MOSFET** (e.g. AO3400 / 2N7002, with a
  100 Ω series gate resistor and a 10 kΩ gate-to-GND pull-down) or **NPN
  transistor** (e.g. S8050 / 2N2222 with a 1 kΩ base resistor), powered from the
  3V3 rail (or VBUS 5V through a series current-limiting resistor sized for the
  3 V motor rating), and place a **flyback diode** (e.g. 1N5819 Schottky or
  1N4148, cathode to `+V`, anode to the motor negative / drain / collector)
  directly across the motor terminals to clamp inductive kickback, plus a 100 nF
  ceramic decoupling capacitor across the motor leads:

  ```text
  3V3 (or 5V + R_lim) ──┬──────────(+) ERM Motor (-)──────────┬── Drain / Collector
                        │                                     │
                     [Diode] 1N5819 (cathode ↑, anode ↓)   [100 nF]
                        │                                     │
                        └─────────────────────────────────────┤
                                                              │
  GP26 (PWM 5A) ──[100 Ω]──┬── Gate / Base      Source / Emitter ── GND
                        [10 kΩ]
                           └── GND
  ```

  ```sh
  env BOARD=waveshare-zero-ili9341-stacked HAPTIC_KIND=pwm \
      cargo build --release -p firmware --features display
  ```

- **TI DRV2605L haptic driver on I2C1 (`HAPTIC_KIND=drv2605`):** drives an ERM or
  LRA actuator through a TI **DRV2605L** (`0x5A`) using its built-in ROM waveform
  library (effect 1 Strong Click 100% for touch, effect 8 Soft Bump 60% ×2 with a
  60 ms pause for the PIN prompt, and effect 14 Strong Buzz 100% held for 450 ms
  for PIN rejection). Default bus pins are **GP14 (I2C1 SDA)** and **GP15 (I2C1
  SCL)** at 400 kHz (`HAPTIC_I2C_SDA=14`, `HAPTIC_I2C_SCL=15`,
  `HAPTIC_I2C_FREQ_HZ=400000`), leaving GP12/GP13 free for UART0 (e.g. an R503
  fingerprint module). The I2C1 peripheral is wrapped in a static
  `embassy_sync::mutex::Mutex<CriticalSectionRawMutex, I2c<'static, I2C1, Blocking>>`
  (`haptic::SharedI2c1`) so a future I2C device (such as an RTC on GP14/GP15) can
  share the bus safely. Select ERM (Library A, default) or LRA (Library F) via
  `actuator = "erm" | "lra"` in `[haptic]` or `HAPTIC_ACTUATOR=erm|lra`:

  | DRV2605L pin | RP2350-Zero | Notes |
  |---|---|---|
  | `VIN` / `GND` | 3V3 / GND | 3.3 V logic and motor supply |
  | `SDA` | GP14 | I2C1 SDA (4.7 kΩ pull-up to 3V3; shared with future RTC) |
  | `SCL` | GP15 | I2C1 SCL (4.7 kΩ pull-up to 3V3; shared with future RTC) |
  | `EN` | 3V3 | tie high (or leave pulled up on breakout boards) |
  | `OUT+` / `OUT-` | Actuator | ERM (`HAPTIC_ACTUATOR=erm`) or LRA (`HAPTIC_ACTUATOR=lra`) |

  ```sh
  # DRV2605L with ERM (default) or LRA actuator
  env BOARD=waveshare-zero-ili9341-stacked HAPTIC_KIND=drv2605 \
      cargo build --release -p firmware --features display
  env BOARD=waveshare-zero-ili9341-stacked HAPTIC_KIND=drv2605 HAPTIC_ACTUATOR=lra \
      cargo build --release -p firmware --features display
  ```

On-device haptic preferences (**Settings → Display → Haptics**) let you adjust
runtime intensity (`Off` / `Low` / `Medium` / `High`), mute **PIN pad clicks** for
acoustic side-channel privacy while keeping prompt and error cues active, and play
a **Test cues** self-test sequence (persisted in `EF_DISPLAY`, `0xE030`).

#### DS3231 I2C1 Real-Time Clock (On-Device TOTP)

A **DS3231** battery-backed I2C real-time clock (`0x68`) can be wired to **GP14
(I2C1 SDA)** and **GP15 (I2C1 SCL)** — sharing `I2C1` cleanly with the DRV2605L
when `HAPTIC_KIND=drv2605` is used (`rtc::SharedI2c1`).

| DS3231 pin | RP2350-Zero | Notes |
|---|---|---|
| `VCC` / `GND` | 3V3 / GND | plus CR2032 / LIR2032 backup cell on the DS3231 module |
| `SDA` | GP14 | I2C1 SDA (shared with DRV2605L when present) |
| `SCL` | GP15 | I2C1 SCL (shared with DRV2605L when present) |

- **Time sync & Oscillator Stop Flag (`OSF`):** At boot (and when opening an OATH
  credential), the firmware checks the DS3231 Status Register (`0x0F` bit 7, `OSF`)
  and verifies the year is `>= 2025`. Sync the device clock from the host with
  `rsk hw --sync-rtc` (or read it with `rsk hw --rtc`); the UTC timestamp is
  written to DS3231 registers `0x00..=0x06` in 24-hour BCD and `OSF` is cleared so
  time survives unplugging:

  ```sh
  rsk hw --sync-rtc
  rsk hw --rtc
  ```

- **Live on-device TOTP:** Opening **Apps → OATH → `<account>`** computes the
  credential's 6–8 digit RFC 6238 TOTP code on-device (`SHA-1`, `SHA-256`, or
  `SHA-512`, custom period prefix) and refreshes the code and countdown once per
  second. Credentials enrolled with `PROP_TOUCH` require a tap on the code card
  (**"Tap to reveal code"**) before computing, and HOTP credentials stay read-only
  so inspecting a detail screen never advances an HOTP counter.

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
