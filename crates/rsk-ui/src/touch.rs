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

/// Screen-space target coordinates for the guided 3-point touch calibration
/// (`Settings → Display → Calibrate touch`):
/// 1. Top-left crosshair (`(24, 32)`)
/// 2. Bottom-right crosshair (`(216, 288)`)
/// 3. Top-right crosshair (`(216, 32)`)
///
/// Followed by a center verification target (`(120, 160)`).
pub const CAL_TARGET_TL: Point = Point::new(24, 32);
pub const CAL_TARGET_BR: Point = Point::new(PANEL_W - 24, PANEL_H - 32);
pub const CAL_TARGET_TR: Point = Point::new(PANEL_W - 24, 32);
pub const CAL_TARGET_CENTER: Point = Point::new(PANEL_W / 2, PANEL_H / 2);

/// Maximum Manhattan distance (per axis, in pixels) around [`CAL_TARGET_CENTER`]
/// that a post-calibration verification tap may land within to confirm the new
/// calibration.
pub const CAL_VERIFY_TOLERANCE_PX: u16 = 28;

/// Minimum raw ADC span between the two opposite corners along each axis during
/// 3-point touch calibration (~25% of full 12-bit scale). Rejects taps where the
/// user tapped the same corner twice.
const CAL_MIN_RAW_SPAN: i32 = 900;

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

/// Filter one XPT2046 burst (pressure check + median filter on each channel)
/// and return the raw 12-bit `(raw_x, raw_y)` ADC medians before any axis swap
/// or panel scaling.
pub fn xpt2046_raw(
    z1: u16,
    z2: u16,
    mut xs: [u16; XPT2046_SAMPLES],
    mut ys: [u16; XPT2046_SAMPLES],
) -> Option<(u16, u16)> {
    let z1 = z1.min(XPT2046_FULL_SCALE);
    if z1 == 0 || (z1 + XPT2046_FULL_SCALE).saturating_sub(z2) < XPT2046_Z_MIN {
        return None;
    }
    Some((steady_median(&mut xs)?, steady_median(&mut ys)?))
}

/// One XPT2046 read — both pressure channels and a burst per axis — as a panel
/// coordinate, or `None` when the press is too light, the burst jitters, or the
/// calibration is degenerate. Out-of-window positions clamp to the panel edge.
pub fn xpt2046_point(
    z1: u16,
    z2: u16,
    xs: [u16; XPT2046_SAMPLES],
    ys: [u16; XPT2046_SAMPLES],
    cal: &Xpt2046Cal,
) -> Option<Point> {
    let (rx, ry) = xpt2046_raw(z1, z2, xs, ys)?;
    xpt2046_map_raw(rx, ry, cal)
}

/// Map a raw 12-bit `(raw_x, raw_y)` pair through `cal` into a panel [`Point`].
pub fn xpt2046_map_raw(rx: u16, ry: u16, cal: &Xpt2046Cal) -> Option<Point> {
    let (rx, ry) = if cal.swap_xy { (ry, rx) } else { (rx, ry) };
    Some(Point::new(
        scale(rx, cal.x_min, cal.x_max, PANEL_W)?,
        scale(ry, cal.y_min, cal.y_max, PANEL_H)?,
    ))
}

/// Derive an [`Xpt2046Cal`] from three raw `(raw_x, raw_y)` taps collected at
/// [`CAL_TARGET_TL`], [`CAL_TARGET_BR`], and [`CAL_TARGET_TR`].
///
/// Comparing the top-left → top-right delta (pure screen +X) against the
/// top-right → bottom-right delta (pure screen +Y) automatically determines
/// whether the XPT2046's X/Y channels are swapped (`swap_xy`), while the two
/// opposite corners extrapolate the full `0..PANEL_W-1` and `0..PANEL_H-1` raw
/// ADC windows (`x_min`, `x_max`, `y_min`, `y_max`), including axis inversion.
/// Returns `None` if the taps are too close together or fails sanity bounds.
pub fn solve_xpt2046_cal(
    raw_tl: (u16, u16),
    raw_br: (u16, u16),
    raw_tr: (u16, u16),
) -> Option<Xpt2046Cal> {
    // From TL to TR, screen X moves across the panel while screen Y is constant.
    // From TR to BR, screen Y moves down the panel while screen X is constant.
    let dx_on_x = (raw_tr.0 as i32 - raw_tl.0 as i32).abs();
    let dy_on_x = (raw_tr.1 as i32 - raw_tl.1 as i32).abs();
    let dx_on_y = (raw_br.0 as i32 - raw_tr.0 as i32).abs();
    let dy_on_y = (raw_br.1 as i32 - raw_tr.1 as i32).abs();

    let swap_xy = if dy_on_x > dx_on_x && dx_on_y > dy_on_y {
        true
    } else if dx_on_x > dy_on_x && dy_on_y > dx_on_y {
        false
    } else {
        return None;
    };

    let (tl_x, tl_y) = if swap_xy {
        (raw_tl.1 as i32, raw_tl.0 as i32)
    } else {
        (raw_tl.0 as i32, raw_tl.1 as i32)
    };
    let (br_x, br_y) = if swap_xy {
        (raw_br.1 as i32, raw_br.0 as i32)
    } else {
        (raw_br.0 as i32, raw_br.1 as i32)
    };

    if (br_x - tl_x).abs() < CAL_MIN_RAW_SPAN || (br_y - tl_y).abs() < CAL_MIN_RAW_SPAN {
        return None;
    }

    let (x_min, x_max) = extrapolate_axis(
        tl_x,
        br_x,
        CAL_TARGET_TL.x as i32,
        CAL_TARGET_BR.x as i32,
        PANEL_W as i32 - 1,
    )?;
    let (y_min, y_max) = extrapolate_axis(
        tl_y,
        br_y,
        CAL_TARGET_TL.y as i32,
        CAL_TARGET_BR.y as i32,
        PANEL_H as i32 - 1,
    )?;

    Some(Xpt2046Cal {
        x_min,
        x_max,
        y_min,
        y_max,
        swap_xy,
    })
}

fn extrapolate_axis(raw1: i32, raw2: i32, px1: i32, px2: i32, last_px: i32) -> Option<(u16, u16)> {
    let dpx = px2 - px1;
    if dpx <= 0 {
        return None;
    }
    let draw = raw2 - raw1;
    let min = (raw1 - (draw * px1) / dpx).clamp(0, XPT2046_FULL_SCALE as i32) as u16;
    let max = (raw1 + (draw * (last_px - px1)) / dpx).clamp(0, XPT2046_FULL_SCALE as i32) as u16;
    if min == max {
        return None;
    }
    Some((min, max))
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
