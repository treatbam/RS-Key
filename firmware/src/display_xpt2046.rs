// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! XPT2046 resistive touch on SPI0, sharing the bus with the ILI9341 panel. This
//! is only the transfer: PENIRQ gates a read, then a pressure pair and a burst per
//! axis go to `rsk_ui::touch`, where the filtering and calibration are host-tested.

use embassy_rp::gpio::{Input, Output};
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Async, Spi};
use rsk_display::TouchPad;
use rsk_ui::touch::{TouchDebounce, XPT2046_SAMPLES, Xpt2046Cal, xpt2046_point};

use super::SharedSpi;

// Control bytes: start bit, channel A2..A0, 12-bit differential, PD=01 (ADC held
// on, PENIRQ off) through the burst. X is A=101, Y is A=001, Z1/Z2 are A=011/100.
const CMD_X: u8 = 0xD1;
const CMD_Y: u8 = 0x91;
const CMD_Z1: u8 = 0xB1;
const CMD_Z2: u8 = 0xC1;
/// The same X conversion with PD=00: power down between conversions, PENIRQ armed.
const CMD_ARM_PENIRQ: u8 = 0xD0;
/// The XPT2046 datasheet caps DCLK at 2.5 MHz; the panel runs far faster.
const TOUCH_SPI_HZ: u32 = 2_000_000;

const CAL: Xpt2046Cal = Xpt2046Cal {
    x_min: crate::BUILD_DISPLAY_TOUCH_X_MIN,
    x_max: crate::BUILD_DISPLAY_TOUCH_X_MAX,
    y_min: crate::BUILD_DISPLAY_TOUCH_Y_MIN,
    y_max: crate::BUILD_DISPLAY_TOUCH_Y_MAX,
    swap_xy: crate::BUILD_DISPLAY_TOUCH_SWAP_XY,
};

/// The touch controller's chip-select and active-low PENIRQ, plus the shared bus.
pub struct TouchHw {
    pub bus: &'static SharedSpi,
    pub cs: Output<'static>,
    pub irq: Input<'static>,
}

pub struct Touch {
    hw: TouchHw,
    debounce: TouchDebounce,
}

impl Touch {
    /// PENIRQ is only driven once a PD=00 conversion has run, so arm it here.
    pub fn new(hw: TouchHw) -> Self {
        let mut touch = Self {
            hw,
            debounce: TouchDebounce::default(),
        };
        touch.with_bus(|spi| {
            convert(spi, CMD_ARM_PENIRQ);
        });
        touch
    }

    /// Run `f` with the touch chip selected at the touch clock, then hand the bus
    /// back at the panel clock. `None` if the bus is busy, which reads as no touch.
    fn with_bus<R>(&mut self, f: impl FnOnce(&mut Spi<'static, SPI0, Async>) -> R) -> Option<R> {
        // Only a panel write in flight holds the lock, and the flow never reads
        // touch from inside one — so this is the transport-error path, not a wait.
        let mut spi = self.hw.bus.try_lock().ok()?;
        spi.set_frequency(TOUCH_SPI_HZ);
        self.hw.cs.set_low();
        let r = f(&mut spi);
        self.hw.cs.set_high();
        spi.set_frequency(crate::BUILD_DISPLAY_SPI_FREQ_HZ);
        Some(r)
    }

    fn sample(&mut self) -> Option<rsk_ui::Point> {
        if self.hw.irq.is_high() {
            return None;
        }
        let (z1, z2, xs, ys) = self.with_bus(|spi| {
            let z1 = convert(spi, CMD_Z1);
            let z2 = convert(spi, CMD_Z2);
            let xs = burst(spi, CMD_X);
            let ys = burst(spi, CMD_Y);
            convert(spi, CMD_ARM_PENIRQ);
            (z1, z2, xs, ys)
        })?;
        xpt2046_point(z1, z2, xs, ys, &CAL)
    }
}

/// One conversion: the control byte, then 16 clocks carrying the 12-bit result
/// left-aligned one bit in (bits 14..3).
fn convert(spi: &mut Spi<'static, SPI0, Async>, cmd: u8) -> u16 {
    let mut buf = [cmd, 0, 0];
    // embassy-rp's SPI error type is uninhabited.
    let _ = spi.blocking_transfer_in_place(&mut buf);
    (u16::from_be_bytes([buf[1], buf[2]]) >> 3) & 0x0FFF
}

/// A burst on one channel. The first conversion after the drivers switch axis
/// samples before the plate settles, so it is discarded.
fn burst(spi: &mut Spi<'static, SPI0, Async>, cmd: u8) -> [u16; XPT2046_SAMPLES] {
    convert(spi, cmd);
    core::array::from_fn(|_| convert(spi, cmd))
}

impl TouchPad for Touch {
    /// One debounced sample: a contact is reported once two reads in a row agree.
    fn read(&mut self) -> Option<rsk_ui::Point> {
        let sample = self.sample();
        self.debounce.update(sample)
    }
}
