// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! `EF_DISPLAY` wire-format codec — the persisted on-device display settings the
//! trusted-display firmware reads at boot and writes when the user edits them in
//! Settings → Display.
//!
//! The block is `[brightness, sleep_secs_be(2), flags]` (4 bytes): the backlight
//! level (`1..=BRIGHTNESS_LEVELS`), the display-sleep timeout in seconds (`0` =
//! Off), and a flags byte ([`FLAG_PIN_DECLINED`] — the user chose "continue without a device
//! PIN" at first-run, so the panel must not re-prompt — [`FLAG_SCRAMBLE_PIN`],
//! [`FLAG_MUTE_PIN_HAPTIC`], and the 2-bit haptic intensity level XORed with
//! [`crate::HAPTIC_LEVEL_DEFAULT`] so a legacy zero-bits record loads at the default
//! Medium intensity).
//! The **touch timeout** is *not* here — it persists in the phy record's
//! `PresenceTimeout` tag (shared with `rsk hw --touch-timeout`), so it keeps one
//! source of truth. Resistive touch calibration persists in its own `EF_TOUCH_CAL`
//! record ([`encode_touch_cal`] / [`decode_touch_cal`]).
//!
//! [`DisplayConfig::apply_block`] overlays a stored block onto a default `self`,
//! field by field, so a record written by an *older* firmware (the original
//! `CORE_LEN`-byte block, no flags) or read by an *older* firmware (a future,
//! longer block — only its known prefix is read) survives a firmware upgrade
//! without losing or misreading a field; anything a shorter block omits keeps its
//! current value. [`DisplayConfig::default`] mirrors the firmware's live defaults,
//! so a device with no record behaves exactly as before.
//!
//! Like `rsk-led`'s codec this crate is pure (no `embassy` / HAL), so the format is
//! unit-testable on the host; `firmware/src/display.rs` owns the live brightness
//! field and the `SLEEP_TIMEOUT_MS` atomic and marshals them through here.

/// `EF_DISPLAY` length: `[brightness, sleep_secs_be(2), flags]`.
pub const CONF_LEN: usize = 4;

/// The original layout (`[brightness, sleep_secs_be(2)]`, no flags byte). A record
/// written before the flags byte existed is exactly this long; [`DisplayConfig::apply_block`]
/// still reads its two fields and leaves the flags at their default, so an already-provisioned
/// device keeps its brightness / sleep across the upgrade that added the byte.
const CORE_LEN: usize = 3;

/// Flags-byte bit 0: the user chose "continue without a device PIN" at the
/// first-run prompt. Set, the panel never re-shows that onboarding screen (until a
/// factory reset wipes `EF_DISPLAY`); clear (the default), a PIN-less device is
/// offered the prompt once.
pub const FLAG_PIN_DECLINED: u8 = 0x01;

/// Flags-byte bit 1: scramble the PIN pad's digits on every entry. Off by default —
/// it trades muscle memory for smudge/over-the-shoulder resistance, and the retry
/// budget makes a typo expensive (`PIN_MISMATCH_LIMIT` is three per power cycle), so
/// it is the owner's call rather than the firmware's.
pub const FLAG_SCRAMBLE_PIN: u8 = 0x02;

/// Flags-byte bit 2: mute haptic touch clicks while the on-screen PIN pad is open
/// (Settings → Display → Haptics → "PIN pad clicks"). Off (clicks enabled) by default.
pub const FLAG_MUTE_PIN_HAPTIC: u8 = 0x04;

/// Flags-byte bits 3..=4: haptic intensity level (`0..=3`), stored XORed with
/// [`crate::HAPTIC_LEVEL_DEFAULT`] (`2`) so `0b00` in an older record decodes to
/// [`crate::HAPTIC_LEVEL_DEFAULT`].
pub const FLAG_HAPTIC_LEVEL_SHIFT: u8 = 3;
/// Mask for the 2-bit haptic level field after shifting by [`FLAG_HAPTIC_LEVEL_SHIFT`].
pub const FLAG_HAPTIC_LEVEL_MASK: u8 = 0x03;

/// Default display-sleep timeout in seconds — the single source of truth: the
/// firmware derives its `DEFAULT_SLEEP_MS` from it, so a device with no record
/// blanks on the same schedule it did before this record existed.
pub const DEFAULT_SLEEP_SECS: u16 = 60;

/// The persisted display settings: backlight level, the display-sleep timeout,
/// first-run PIN-prompt flag, PIN-pad scramble flag, and haptic preferences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DisplayConfig {
    /// Backlight level `1..=BRIGHTNESS_LEVELS`. Stored raw; the firmware clamps it
    /// to the valid range when it applies it to the PWM, so a corrupt or
    /// out-of-range byte can never blank or over-drive the panel.
    pub brightness: u8,
    /// Display-sleep timeout in seconds; `0` = Off (never blanks).
    pub sleep_secs: u16,
    /// The user has dismissed the first-run "set a device PIN?" prompt by choosing
    /// to continue without one — so the panel must not re-offer it. Cleared on a
    /// factory reset (which wipes the record), so a wiped device re-onboards.
    pub pin_declined: bool,
    /// Draw the PIN pad's digits in a fresh random order for every entry (Settings →
    /// Security → "Scramble PIN pad"). Off by default.
    pub scramble_pin: bool,
    /// Haptic intensity level (`0` = Off, `1` = Low, `2` = Medium default, `3` = High).
    pub haptic_level: u8,
    /// Suppress haptic touch clicks while the on-screen PIN pad is open.
    pub mute_pin_haptic: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            brightness: crate::BRIGHTNESS_LEVELS,
            sleep_secs: DEFAULT_SLEEP_SECS,
            pin_declined: false,
            scramble_pin: false,
            haptic_level: crate::HAPTIC_LEVEL_DEFAULT,
            mute_pin_haptic: false,
        }
    }
}

impl DisplayConfig {
    /// Pack into the 4-byte wire block: `[brightness, sleep_secs_be, flags]`
    /// (big-endian sleep, matching the phy record's byte order).
    pub fn encode(&self) -> [u8; CONF_LEN] {
        let s = self.sleep_secs.to_be_bytes();
        let mut flags = 0;
        if self.pin_declined {
            flags |= FLAG_PIN_DECLINED;
        }
        if self.scramble_pin {
            flags |= FLAG_SCRAMBLE_PIN;
        }
        if self.mute_pin_haptic {
            flags |= FLAG_MUTE_PIN_HAPTIC;
        }
        let level_bits = (self.haptic_level & FLAG_HAPTIC_LEVEL_MASK) ^ crate::HAPTIC_LEVEL_DEFAULT;
        flags |= (level_bits & FLAG_HAPTIC_LEVEL_MASK) << FLAG_HAPTIC_LEVEL_SHIFT;
        [self.brightness, s[0], s[1], flags]
    }

    /// Overlay a stored block onto `self`, field by field. The brightness + sleep
    /// pair is read from any block at least `CORE_LEN` long (so the original
    /// flags-less record still loads, keeping `pin_declined` at its default); the
    /// flags byte is read only from a full [`CONF_LEN`] block. A future, longer
    /// block is read up to its known prefix. Anything shorter than `CORE_LEN` can
    /// only be flash corruption, so those fields stay at their defaults rather than
    /// half-applied.
    pub fn apply_block(&mut self, b: &[u8]) {
        if b.len() >= CORE_LEN {
            self.brightness = b[0];
            self.sleep_secs = u16::from_be_bytes([b[1], b[2]]);
        }
        if b.len() >= CONF_LEN {
            self.pin_declined = b[3] & FLAG_PIN_DECLINED != 0;
            self.scramble_pin = b[3] & FLAG_SCRAMBLE_PIN != 0;
            self.mute_pin_haptic = b[3] & FLAG_MUTE_PIN_HAPTIC != 0;
            let raw_level = (b[3] >> FLAG_HAPTIC_LEVEL_SHIFT) & FLAG_HAPTIC_LEVEL_MASK;
            self.haptic_level = raw_level ^ crate::HAPTIC_LEVEL_DEFAULT;
        }
    }
}

/// `EF_TOUCH_CAL` wire length: `[flags, x_min_be(2), x_max_be(2), y_min_be(2), y_max_be(2)]`.
pub const TOUCH_CAL_LEN: usize = 9;

/// `EF_TOUCH_CAL` flag bit 0: swap raw X/Y axes before mapping to panel coordinates.
pub const TOUCH_CAL_FLAG_SWAP_XY: u8 = 0x01;

/// Encode an [`Xpt2046Cal`](crate::touch::Xpt2046Cal) into its 9-byte flash record.
pub fn encode_touch_cal(cal: &crate::touch::Xpt2046Cal) -> [u8; TOUCH_CAL_LEN] {
    let x0 = cal.x_min.to_be_bytes();
    let x1 = cal.x_max.to_be_bytes();
    let y0 = cal.y_min.to_be_bytes();
    let y1 = cal.y_max.to_be_bytes();
    let flags = if cal.swap_xy {
        TOUCH_CAL_FLAG_SWAP_XY
    } else {
        0
    };
    [
        flags, x0[0], x0[1], x1[0], x1[1], y0[0], y0[1], y1[0], y1[1],
    ]
}

/// Decode a stored `EF_TOUCH_CAL` record, rejecting truncated, zero-span, or out-of-12-bit-ADC
/// records so flash corruption can never install a degenerate touch mapping.
pub fn decode_touch_cal(b: &[u8]) -> Option<crate::touch::Xpt2046Cal> {
    if b.len() < TOUCH_CAL_LEN {
        return None;
    }
    let swap_xy = b[0] & TOUCH_CAL_FLAG_SWAP_XY != 0;
    let x_min = u16::from_be_bytes([b[1], b[2]]);
    let x_max = u16::from_be_bytes([b[3], b[4]]);
    let y_min = u16::from_be_bytes([b[5], b[6]]);
    let y_max = u16::from_be_bytes([b[7], b[8]]);
    if x_min > 4095 || x_max > 4095 || y_min > 4095 || y_max > 4095 {
        return None;
    }
    if (x_min as i32 - x_max as i32).abs() < 400 || (y_min as i32 - y_max as i32).abs() < 400 {
        return None;
    }
    Some(crate::touch::Xpt2046Cal {
        x_min,
        x_max,
        y_min,
        y_max,
        swap_xy,
    })
}

#[cfg(kani)]
#[path = "settings_store_kani.rs"]
mod proofs;

#[cfg(test)]
#[path = "settings_store_tests.rs"]
mod tests;
