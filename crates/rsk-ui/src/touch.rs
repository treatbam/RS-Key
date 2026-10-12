// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! Turning a touch controller's raw report into a panel coordinate: a CST328
//! capacitive report, or a burst of XPT2046 resistive ADC samples. Pure and
//! host-tested: the firmware does the bus transfer and hands the raw values here,
//! so the arithmetic that turns them into the point [`crate::hit_confirm`] will
//! judge sits under test rather than buried in a driver — a wrong reconstruction
//! maps a tap to the wrong button.

use crate::{PANEL_H, PANEL_W, Point};

/// Finger-1 status nibble (CST328 register 0xD000, low 4 bits) meaning a finger
/// is down.
const FINGER_DOWN: u8 = 0x06;

/// Parse a CST328 report block read starting at register 0xD000, returning the
/// first active finger's coordinate or `None` (no finger down, or a short block).
///
/// Byte layout returned by the controller from 0xD000: `[0]` finger-1 status,
/// `[1]` X high 8 bits, `[2]` Y high 8 bits, `[3]` X low 4 bits (upper nibble) |
/// Y low 4 bits (lower nibble). Both axes are 12-bit. The controller is configured
/// at the panel resolution and reports the panel's own axes, so `display.rs` hands
/// this straight on — there is no rotation or flip step anywhere.
pub fn parse_cst328(report: &[u8]) -> Option<Point> {
    if report.len() < 4 || report[0] & 0x0F != FINGER_DOWN {
        return None;
    }
    let x = ((report[1] as u16) << 4) | ((report[3] as u16) >> 4);
    let y = ((report[2] as u16) << 4) | ((report[3] as u16) & 0x0F);
    Some(Point::new(x, y))
}

/// The XPT2046's 12-bit ADC full scale.
const XPT2046_FULL_SCALE: u16 = 0x0FFF;

/// Samples per axis in one XPT2046 read. Odd, so the median is a real sample.
pub const XPT2046_SAMPLES: usize = 5;

/// Least pressure (`z1 + 4095 − z2`, larger is firmer) that counts as a contact.
/// Below it the plates barely touch and the position reads are noise.
const XPT2046_Z_MIN: u16 = 400;

/// Widest spread of the middle three sorted samples, in raw counts (~3 px at
/// the default calibration). Wider means a finger sliding or lifting mid-burst.
const XPT2046_MAX_SPREAD: u16 = 48;

/// How far two consecutive accepted reads may be apart, per axis, and still be
/// one steady contact.
const DEBOUNCE_PX: u16 = 12;

/// The raw-ADC window that maps onto the panel. A `min` above its `max` flips
/// that axis; `swap_xy` feeds panel x from the Y channel (and y from X).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Xpt2046Cal {
    pub x_min: u16,
    pub x_max: u16,
    pub y_min: u16,
    pub y_max: u16,
    pub swap_xy: bool,
}

/// One XPT2046 read — both pressure channels and a burst per axis — as a panel
/// coordinate, or `None` when the press is too light, the burst jitters, or the
/// calibration is degenerate. Out-of-window positions clamp to the panel edge.
pub fn xpt2046_point(
    z1: u16,
    z2: u16,
    mut xs: [u16; XPT2046_SAMPLES],
    mut ys: [u16; XPT2046_SAMPLES],
    cal: &Xpt2046Cal,
) -> Option<Point> {
    let z1 = z1.min(XPT2046_FULL_SCALE);
    if z1 == 0 || (z1 + XPT2046_FULL_SCALE).saturating_sub(z2) < XPT2046_Z_MIN {
        return None;
    }
    let (rx, ry) = (steady_median(&mut xs)?, steady_median(&mut ys)?);
    let (rx, ry) = if cal.swap_xy { (ry, rx) } else { (rx, ry) };
    Some(Point::new(
        scale(rx, cal.x_min, cal.x_max, PANEL_W)?,
        scale(ry, cal.y_min, cal.y_max, PANEL_H)?,
    ))
}

fn steady_median(s: &mut [u16; XPT2046_SAMPLES]) -> Option<u16> {
    s.sort_unstable();
    let mid = XPT2046_SAMPLES / 2;
    (s[mid + 1] - s[mid - 1] <= XPT2046_MAX_SPREAD).then_some(s[mid])
}

fn scale(raw: u16, min: u16, max: u16, span: u16) -> Option<u16> {
    let (raw, min, max, last) = (
        i32::from(raw),
        i32::from(min),
        i32::from(max),
        i32::from(span) - 1,
    );
    if min == max {
        return None;
    }
    Some(((raw - min) * last / (max - min)).clamp(0, last) as u16)
}

/// Reports a contact only once two consecutive reads agree, so a single noisy
/// sample — common as resistive plates first meet — never becomes a tap.
#[derive(Default)]
pub struct TouchDebounce {
    last: Option<Point>,
}

impl TouchDebounce {
    /// Feed one read; returns the point to report, if any.
    pub fn update(&mut self, sample: Option<Point>) -> Option<Point> {
        match (core::mem::replace(&mut self.last, sample), sample) {
            (Some(a), Some(b))
                if a.x.abs_diff(b.x) <= DEBOUNCE_PX && a.y.abs_diff(b.y) <= DEBOUNCE_PX =>
            {
                Some(b)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "touch_tests.rs"]
mod tests;
