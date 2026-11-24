// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

use super::*;

#[test]
fn reconstructs_finger_coordinate() {
    // x=100 (0x064): X high 8 = 0x06, X low 4 = 0x4. y=200 (0x0C8): Y high 8 =
    // 0x0C, Y low 4 = 0x8 — so the packed low-nibble byte is 0x48.
    let r = [FINGER_DOWN, 0x06, 0x0C, 0x48, 0, 0, 0];
    assert_eq!(parse_cst328(&r), Some(Point::new(100, 200)));
}

#[test]
fn no_finger_down_is_none() {
    assert_eq!(parse_cst328(&[0x00, 0x06, 0x0C, 0x48]), None);
}

#[test]
fn short_report_is_none() {
    assert_eq!(parse_cst328(&[FINGER_DOWN, 0x06]), None);
    assert_eq!(parse_cst328(&[]), None);
}

#[test]
fn full_scale_is_twelve_bits_each() {
    let r = [FINGER_DOWN, 0xFF, 0xFF, 0xFF];
    assert_eq!(parse_cst328(&r), Some(Point::new(0xFFF, 0xFFF)));
}

const CAL: Xpt2046Cal = Xpt2046Cal {
    x_min: 300,
    x_max: 3800,
    y_min: 300,
    y_max: 3800,
    swap_xy: false,
};
/// A firm press (z = 1000 + 4095 − 3000 = 2095).
const Z1: u16 = 1000;
const Z2: u16 = 3000;

fn burst(v: u16) -> [u16; XPT2046_SAMPLES] {
    [v; XPT2046_SAMPLES]
}

#[test]
fn xpt2046_maps_window_corners_to_panel_corners() {
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(300), burst(300), &CAL),
        Some(Point::new(0, 0))
    );
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(3800), burst(3800), &CAL),
        Some(Point::new(PANEL_W - 1, PANEL_H - 1))
    );
}

#[test]
fn xpt2046_clamps_outside_the_window() {
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(0), burst(4095), &CAL),
        Some(Point::new(0, PANEL_H - 1))
    );
}

#[test]
fn xpt2046_inverted_window_flips_the_axis() {
    let cal = Xpt2046Cal {
        x_min: 3800,
        x_max: 300,
        ..CAL
    };
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(3800), burst(300), &cal),
        Some(Point::new(0, 0))
    );
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(300), burst(300), &cal),
        Some(Point::new(PANEL_W - 1, 0))
    );
}

#[test]
fn xpt2046_swap_feeds_x_from_the_y_channel() {
    let cal = Xpt2046Cal {
        swap_xy: true,
        ..CAL
    };
    // Raw X at the window floor, raw Y at its ceiling: swapped, x is far right.
    assert_eq!(
        xpt2046_point(Z1, Z2, burst(300), burst(3800), &cal),
        Some(Point::new(PANEL_W - 1, 0))
    );
}

#[test]
fn xpt2046_median_ignores_one_outlier_each_side() {
    let xs = [2050, 4000, 2040, 0, 2045];
    assert_eq!(
        xpt2046_point(Z1, Z2, xs, burst(300), &CAL),
        xpt2046_point(Z1, Z2, burst(2045), burst(300), &CAL)
    );
}

#[test]
fn xpt2046_rejects_a_jittery_burst() {
    let xs = [1000, 1100, 1200, 1300, 1400];
    assert_eq!(xpt2046_point(Z1, Z2, xs, burst(300), &CAL), None);
}

#[test]
fn xpt2046_rejects_a_light_press() {
    // z = 100 + 4095 − 4000 = 195, under the floor.
    assert_eq!(
        xpt2046_point(100, 4000, burst(2000), burst(2000), &CAL),
        None
    );
    assert_eq!(xpt2046_point(0, 0, burst(2000), burst(2000), &CAL), None);
}

#[test]
fn xpt2046_degenerate_calibration_is_none() {
    let cal = Xpt2046Cal { x_max: 300, ..CAL };
    assert_eq!(xpt2046_point(Z1, Z2, burst(2000), burst(2000), &cal), None);
}

#[test]
fn debounce_needs_two_agreeing_reads() {
    let mut d = TouchDebounce::default();
    let p = Point::new(100, 100);
    assert_eq!(d.update(Some(p)), None);
    assert_eq!(
        d.update(Some(Point::new(105, 95))),
        Some(Point::new(105, 95))
    );
    assert_eq!(d.update(None), None);
    assert_eq!(d.update(Some(p)), None);
}

#[test]
fn debounce_drops_a_jump() {
    let mut d = TouchDebounce::default();
    assert_eq!(d.update(Some(Point::new(10, 10))), None);
    assert_eq!(d.update(Some(Point::new(200, 10))), None);
    assert_eq!(
        d.update(Some(Point::new(200, 12))),
        Some(Point::new(200, 12))
    );
}

#[test]
fn solve_xpt2046_cal_recovers_direct_and_swapped_axes() {
    for swap in [false, true] {
        for inv_x in [false, true] {
            for inv_y in [false, true] {
                let cal = Xpt2046Cal {
                    x_min: if inv_x { 3820 } else { 240 },
                    x_max: if inv_x { 240 } else { 3820 },
                    y_min: if inv_y { 3880 } else { 280 },
                    y_max: if inv_y { 280 } else { 3880 },
                    swap_xy: swap,
                };
                // Synthesize raw ADC readings at the three calibration targets.
                let raw_for = |target: Point| -> (u16, u16) {
                    let rx = cal.x_min as i32
                        + (target.x as i32 * (cal.x_max as i32 - cal.x_min as i32))
                            / (PANEL_W as i32 - 1);
                    let ry = cal.y_min as i32
                        + (target.y as i32 * (cal.y_max as i32 - cal.y_min as i32))
                            / (PANEL_H as i32 - 1);
                    if swap {
                        (ry as u16, rx as u16)
                    } else {
                        (rx as u16, ry as u16)
                    }
                };
                let tl = raw_for(CAL_TARGET_TL);
                let br = raw_for(CAL_TARGET_BR);
                let tr = raw_for(CAL_TARGET_TR);
                let solved = solve_xpt2046_cal(tl, br, tr).expect("valid 3-point calibration");
                assert_eq!(solved.swap_xy, swap);
                let center_raw = raw_for(CAL_TARGET_CENTER);
                let mapped =
                    xpt2046_map_raw(center_raw.0, center_raw.1, &solved).expect("mapped center");
                assert!(
                    mapped.x.abs_diff(CAL_TARGET_CENTER.x) <= 2
                        && mapped.y.abs_diff(CAL_TARGET_CENTER.y) <= 2,
                    "mapped center {mapped:?} drifted from {CAL_TARGET_CENTER:?}"
                );
            }
        }
    }
    // Degenerate identical taps are rejected.
    assert_eq!(
        solve_xpt2046_cal((2000, 2000), (2000, 2000), (2000, 2000)),
        None
    );
}
