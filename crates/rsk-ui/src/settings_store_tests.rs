// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

use super::*;

#[test]
fn conf_len_matches_layout() {
    assert_eq!(CONF_LEN, 4);
    assert_eq!(CORE_LEN, 3);
}

#[test]
fn default_mirrors_firmware_runtime_defaults() {
    let d = DisplayConfig::default();
    assert_eq!(d.brightness, crate::BRIGHTNESS_LEVELS);
    assert_eq!(d.sleep_secs, 60);
    assert!(!d.pin_declined);
    assert!(!d.scramble_pin);
    assert_eq!(d.haptic_level, crate::HAPTIC_LEVEL_DEFAULT);
    assert!(!d.mute_pin_haptic);
}

#[test]
fn encode_decode_roundtrip() {
    let cfg = DisplayConfig {
        brightness: 3,
        sleep_secs: 120,
        pin_declined: true,
        scramble_pin: false,
        haptic_level: crate::HAPTIC_LEVEL_HIGH,
        mute_pin_haptic: true,
    };
    let mut got = DisplayConfig::default();
    got.apply_block(&cfg.encode());
    assert_eq!(got, cfg);
}

#[test]
fn encode_layout_is_brightness_then_be_sleep_then_flags() {
    let b = DisplayConfig {
        brightness: 4,
        sleep_secs: 300,
        pin_declined: true,
        scramble_pin: false,
        ..DisplayConfig::default()
    }
    .encode();
    assert_eq!(b.len(), 4);
    assert_eq!(b[0], 4);
    assert_eq!(u16::from_be_bytes([b[1], b[2]]), 300);
    assert_eq!(b[3], FLAG_PIN_DECLINED);
}

#[test]
fn pin_declined_clear_encodes_zero_flags() {
    let b = DisplayConfig {
        brightness: 2,
        sleep_secs: 60,
        pin_declined: false,
        scramble_pin: false,
        ..DisplayConfig::default()
    }
    .encode();
    assert_eq!(b[3], 0);
}

#[test]
fn off_sentinel_roundtrips() {
    let cfg = DisplayConfig {
        brightness: 5,
        sleep_secs: 0, // Off
        pin_declined: false,
        scramble_pin: false,
        ..DisplayConfig::default()
    };
    let mut got = DisplayConfig::default();
    got.apply_block(&cfg.encode());
    assert_eq!(got, cfg);
}

#[test]
fn legacy_core_block_loads_fields_and_keeps_flag_default() {
    // A record written before the flags byte existed is exactly CORE_LEN: its
    // brightness + sleep still load, and pin_declined stays at its default
    // (false) — so an already-provisioned device survives the upgrade.
    let core = [3u8, 0x00, 0x1E]; // brightness 3, sleep 30 s, no flags
    let mut got = DisplayConfig::default();
    got.apply_block(&core);
    assert_eq!(got.brightness, 3);
    assert_eq!(got.sleep_secs, 30);
    assert!(!got.pin_declined);
    assert_eq!(got.haptic_level, crate::HAPTIC_LEVEL_DEFAULT);
    assert!(!got.mute_pin_haptic);
}

#[test]
fn unknown_flag_bits_do_not_set_pin_declined() {
    // Only bit 0 is FLAG_PIN_DECLINED; a future bit set alone must not be read
    // as a decline.
    let mut got = DisplayConfig::default();
    got.apply_block(&[5, 0x00, 0x3C, 0x02]);
    assert!(!got.pin_declined);
}

#[test]
fn empty_block_keeps_defaults() {
    let mut got = DisplayConfig::default();
    got.apply_block(&[]);
    assert_eq!(got, DisplayConfig::default());
}

#[test]
fn short_block_below_core_len_keeps_defaults() {
    // No sub-CORE_LEN layout was ever written, so a 1- or 2-byte block is
    // corruption: leave the fields at their defaults rather than half-apply.
    for short in [&[2u8][..], &[2u8, 0u8][..]] {
        let mut got = DisplayConfig::default();
        got.apply_block(short);
        assert_eq!(got, DisplayConfig::default());
    }
}

#[test]
fn longer_future_block_reads_known_prefix() {
    // A future, longer record still loads its first CONF_LEN bytes (the >= branch).
    let cfg = DisplayConfig {
        brightness: 1,
        sleep_secs: 30,
        pin_declined: true,
        scramble_pin: false,
        ..DisplayConfig::default()
    };
    let mut b = [0u8; 7];
    b[..CONF_LEN].copy_from_slice(&cfg.encode());
    let mut got = DisplayConfig::default();
    got.apply_block(&b);
    assert_eq!(got, cfg);
}

/// Deterministic property sweep: `apply_block` must never panic and must be
/// idempotent on an *arbitrary* byte slice — the persisted record is attacker-
/// or corruption-influenced flash, and `Storage::read` returns the value's *full*
/// length (which can exceed the firmware's 4-byte read buffer), so the codec is
/// the chokepoint that guarantees an out-of-range, truncated, all-0xFF, empty, or
/// over-long record can only mis-load fields, never index OOB or brick the boot.
#[test]
fn apply_block_no_panic_and_idempotent_over_random_slices() {
    // A tiny xorshift64* PRNG: deterministic, reproducible in CI, no dev-dep.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    // Lengths from 0 up to well past CONF_LEN exercise: empty, sub-CORE_LEN,
    // CORE_LEN-only (legacy), full CONF_LEN, and the longer-than-known cases —
    // the last is what a host buffer trim (`n.min(buf.len())`) protects in
    // `Ui::build`; proving the codec itself is OOB-safe makes that defence
    // belt-and-braces, not the only thing standing between flash and a panic.
    let mut bytes = [0u8; 64];
    for _ in 0..50_000 {
        let len = (next() as usize) % (bytes.len() + 1);
        for b in bytes[..len].iter_mut() {
            *b = next() as u8;
        }
        let block = &bytes[..len];

        // (a) panic-freedom: applying onto a fresh default must not index OOB.
        let mut a = DisplayConfig::default();
        a.apply_block(block); // would panic here on any OOB bug

        // (b) idempotence: a second application of the *same* block is a no-op,
        // because apply_block is a pure field-by-field overlay (no accumulation).
        let mut b2 = a;
        b2.apply_block(block);
        assert_eq!(a, b2, "apply_block not idempotent for {block:02x?}");

        // Cross-check the documented prefix contract: a >= CORE_LEN block sets
        // brightness/sleep from the first 3 bytes; a shorter one leaves them at
        // the default (never half-applied); the flag only loads from a full block.
        if len >= CORE_LEN {
            assert_eq!(a.brightness, block[0]);
            assert_eq!(a.sleep_secs, u16::from_be_bytes([block[1], block[2]]));
        } else {
            assert_eq!(a.brightness, DisplayConfig::default().brightness);
            assert_eq!(a.sleep_secs, DisplayConfig::default().sleep_secs);
        }
        if len >= CONF_LEN {
            assert_eq!(a.pin_declined, block[3] & FLAG_PIN_DECLINED != 0);
        } else {
            assert!(!a.pin_declined); // default
        }
    }
}

/// The all-0xFF block (erased / never-written flash reads as 0xFF on this medium)
/// must load cleanly, not panic: brightness 0xFF (the firmware clamps it to the
/// valid range before driving the PWM), sleep 0xFFFF, pin_declined set.
#[test]
fn all_ones_block_loads_without_panic() {
    let mut got = DisplayConfig::default();
    got.apply_block(&[0xFF; CONF_LEN]);
    assert_eq!(got.brightness, 0xFF); // raw; firmware clamps to 1..=BRIGHTNESS_LEVELS
    assert_eq!(got.sleep_secs, 0xFFFF);
    assert!(got.pin_declined);
}

/// The scramble bit rides the flags byte that already existed, so the record does not
/// grow and an already-provisioned device keeps every field across the upgrade. Both
/// flags are independent: a device that dismissed onboarding must not come back with a
/// scrambled pad it never asked for.
#[test]
fn the_scramble_flag_is_independent_of_the_onboarding_flag() {
    for (declined, scramble) in [(false, false), (false, true), (true, false), (true, true)] {
        let cfg = DisplayConfig {
            brightness: 3,
            sleep_secs: 45,
            pin_declined: declined,
            scramble_pin: scramble,
            ..DisplayConfig::default()
        };
        let mut back = DisplayConfig::default();
        back.apply_block(&cfg.encode());
        assert_eq!(back, cfg, "declined={declined} scramble={scramble}");
    }
}

/// A record written before this bit existed — the flags-less `CORE_LEN` block and a
/// full block with only the onboarding bit set — must load with scrambling OFF. It is
/// opt-in, and a firmware upgrade is not the owner asking for it.
#[test]
fn an_older_record_loads_with_scrambling_off() {
    for block in [&[4u8, 0, 60][..], &[4, 0, 60, FLAG_PIN_DECLINED][..]] {
        let mut cfg = DisplayConfig::default();
        cfg.apply_block(block);
        assert!(!cfg.scramble_pin, "block {block:?} turned scrambling on");
        assert_eq!(cfg.brightness, 4);
        assert_eq!(cfg.sleep_secs, 60);
    }
}

/// And the other direction: an older firmware reading a record this one wrote sees the
/// fields it knows, because the bit went into a spare bit of a byte that was already
/// there rather than lengthening the block.
#[test]
fn an_older_firmware_still_reads_a_record_carrying_the_new_bit() {
    let block = DisplayConfig {
        brightness: 2,
        sleep_secs: 30,
        pin_declined: true,
        scramble_pin: true,
        ..DisplayConfig::default()
    }
    .encode();
    assert_eq!(block.len(), CONF_LEN, "the record must not have grown");
    assert_eq!(block[0], 2);
    assert_eq!(u16::from_be_bytes([block[1], block[2]]), 30);
    assert_eq!(block[3] & FLAG_PIN_DECLINED, FLAG_PIN_DECLINED);
}

#[test]
fn haptic_level_and_pin_mute_roundtrip_all_combinations() {
    for level in 0..=crate::HAPTIC_LEVEL_HIGH {
        for mute in [false, true] {
            for declined in [false, true] {
                for scramble in [false, true] {
                    let cfg = DisplayConfig {
                        brightness: 3,
                        sleep_secs: 60,
                        pin_declined: declined,
                        scramble_pin: scramble,
                        haptic_level: level,
                        mute_pin_haptic: mute,
                    };
                    let mut back = DisplayConfig::default();
                    back.apply_block(&cfg.encode());
                    assert_eq!(back, cfg);
                }
            }
        }
    }
}

#[test]
fn touch_cal_encode_decode_roundtrips_and_rejects_degenerate() {
    let cal = crate::touch::Xpt2046Cal {
        x_min: 280,
        x_max: 3780,
        y_min: 3920,
        y_max: 240,
        swap_xy: true,
    };
    let enc = encode_touch_cal(&cal);
    assert_eq!(decode_touch_cal(&enc), Some(cal));

    // Truncated, all-zero, all-0xFF, and small-span blocks are rejected.
    assert_eq!(decode_touch_cal(&enc[..TOUCH_CAL_LEN - 1]), None);
    assert_eq!(decode_touch_cal(&[0u8; TOUCH_CAL_LEN]), None);
    assert_eq!(decode_touch_cal(&[0xFF; TOUCH_CAL_LEN]), None);
    let tiny = crate::touch::Xpt2046Cal {
        x_min: 1000,
        x_max: 1100,
        ..cal
    };
    assert_eq!(decode_touch_cal(&encode_touch_cal(&tiny)), None);
}
