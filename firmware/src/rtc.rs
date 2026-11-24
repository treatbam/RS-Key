// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! DS3231 I2C real-time clock driver on `I2C1` (default GPIO14 SDA / GPIO15 SCL,
//! 7-bit address `0x68`), shared cleanly with the DRV2605L haptic driver when
//! `HAPTIC_KIND=drv2605`.
//!
//! When present and its Oscillator Stop Flag (`OSF`, Status Register `0x0F` bit 7)
//! is clear, the DS3231 seeds the firmware's UTC Unix epoch at boot and serves
//! live on-device TOTP codes on the trusted display. When the host sets the device
//! clock via [`crate::rescue_platform::RescuePlatform::set_time`] (`rsk`), the
//! new UTC timestamp is written through to the DS3231 registers `0x00..=0x06`
//! and `OSF` is cleared so time survives power cycles.

use embassy_rp::{
    i2c::{Blocking as I2cBlocking, I2c},
    peripherals::I2C1,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

/// Shared `I2C1` bus mutex on GP14 (SDA) / GP15 (SCL) for the DS3231 RTC and
/// the optional DRV2605L haptic driver.
pub type SharedI2c1 = Mutex<CriticalSectionRawMutex, I2c<'static, I2C1, I2cBlocking>>;

const DS3231_ADDR: u8 = 0x68;
const REG_SECONDS: u8 = 0x00;
const REG_STATUS: u8 = 0x0F;
const STATUS_OSF: u8 = 0x80;

/// Minimum plausible UTC timestamp (2025-01-01T00:00:00Z) — a factory-default
/// DS3231 powers up at `2000-01-01` with `OSF = 1`, so any year below 2025 is
/// treated as an unset clock even if `OSF` was cleared without setting the date.
const MIN_VALID_UNIX_SECS: u64 = 1_735_689_600;

static I2C_BUS: Mutex<CriticalSectionRawMutex, Option<&'static SharedI2c1>> = Mutex::new(None);

/// Register the initialized `I2C1` bus so the DS3231 RTC can be probed and
/// updated from the thread executor.
#[cfg_attr(all(feature = "display", display_panel = "st7789"), allow(dead_code))]
pub fn init_bus(bus: &'static SharedI2c1) {
    if let Ok(mut guard) = I2C_BUS.try_lock() {
        *guard = Some(bus);
    }
}

#[inline]
const fn bcd_to_bin(bcd: u8) -> Option<u8> {
    let hi = bcd >> 4;
    let lo = bcd & 0x0F;
    if hi <= 9 && lo <= 9 {
        Some(hi * 10 + lo)
    } else {
        None
    }
}

#[inline]
const fn bin_to_bcd(bin: u8) -> u8 {
    ((bin / 10) << 4) | (bin % 10)
}

const fn is_leap_year(y: u16) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

const fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Convert a UTC `(year, month 1..=12, day 1..=31, hour, min, sec)` tuple from
/// the DS3231 (`2000..=2199`) into Unix epoch seconds.
pub const fn ymd_hms_to_unix(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    min: u8,
    sec: u8,
) -> Option<u64> {
    if year < 2000
        || year > 2199
        || month < 1
        || month > 12
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || min > 59
        || sec > 59
    {
        return None;
    }
    // Days from 1970-01-01 to 2000-01-01 is 10957.
    let mut days: u64 = 10_957;
    let mut y = 2000u16;
    while y < year {
        days += if is_leap_year(y) { 366 } else { 365 };
        y += 1;
    }
    let mut m = 1u8;
    while m < month {
        days += days_in_month(year, m) as u64;
        m += 1;
    }
    days += (day - 1) as u64;
    Some(days * 86_400 + (hour as u64) * 3_600 + (min as u64) * 60 + (sec as u64))
}

/// Convert Unix epoch seconds (`>= 2000-01-01T00:00:00Z`) into UTC
/// `(year, month 1..=12, day 1..=31, weekday 1..=7, hour, min, sec)`.
pub const fn unix_to_ymd_hms(unix_secs: u64) -> Option<(u16, u8, u8, u8, u8, u8, u8)> {
    // 2000-01-01T00:00:00Z = 946_684_800.
    if unix_secs < 946_684_800 {
        return None;
    }
    let rem_since_2000 = unix_secs - 946_684_800;
    let mut days = rem_since_2000 / 86_400;
    let sod = rem_since_2000 % 86_400;
    let hour = (sod / 3_600) as u8;
    let min = ((sod % 3_600) / 60) as u8;
    let sec = (sod % 60) as u8;
    // 2000-01-01 was Saturday (6 in 1..=7 where Sunday=1).
    let wday = (((days + 6) % 7) as u8) + 1;

    let mut year = 2000u16;
    loop {
        let yd = if is_leap_year(year) { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
        if year > 2199 {
            return None;
        }
    }
    let mut month = 1u8;
    while month <= 12 {
        let md = days_in_month(year, month) as u64;
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = (days as u8) + 1;
    Some((year, month, day, wday, hour, min, sec))
}

/// Read the DS3231 RTC over `I2C1` without blocking if the bus is busy.
/// Returns `None` if no DS3231 is wired on `I2C1`, if the Oscillator Stop Flag
/// (`OSF`) is set, or if the stored date is earlier than 2025.
pub fn read_unix_secs() -> Option<u64> {
    let bus = (*I2C_BUS.try_lock().ok()?)?;
    let mut i2c = bus.try_lock().ok()?;

    let mut status = [0u8; 1];
    i2c.blocking_write_read(DS3231_ADDR, &[REG_STATUS], &mut status)
        .ok()?;
    if status[0] & STATUS_OSF != 0 {
        return None;
    }

    let mut regs = [0u8; 7];
    i2c.blocking_write_read(DS3231_ADDR, &[REG_SECONDS], &mut regs)
        .ok()?;

    let sec = bcd_to_bin(regs[0] & 0x7F)?;
    let min = bcd_to_bin(regs[1] & 0x7F)?;
    let hour = if regs[2] & 0x40 != 0 {
        // 12-hour mode: bit 5 is PM, bits 4..0 are 01..12 BCD.
        let h12 = bcd_to_bin(regs[2] & 0x1F)?;
        if !(1..=12).contains(&h12) {
            return None;
        }
        let pm = regs[2] & 0x20 != 0;
        match (h12, pm) {
            (12, false) => 0,
            (12, true) => 12,
            (h, true) => h + 12,
            (h, false) => h,
        }
    } else {
        bcd_to_bin(regs[2] & 0x3F)?
    };
    let day = bcd_to_bin(regs[4] & 0x3F)?;
    let century = (regs[5] & 0x80 != 0) as u16;
    let month = bcd_to_bin(regs[5] & 0x1F)?;
    let yr = bcd_to_bin(regs[6])? as u16;
    let year = 2000 + century * 100 + yr;

    let unix = ymd_hms_to_unix(year, month, day, hour, min, sec)?;
    (unix >= MIN_VALID_UNIX_SECS).then_some(unix)
}

/// Write a UTC Unix timestamp (`>= 2025-01-01T00:00:00Z`) to the DS3231 RTC in
/// 24-hour mode and clear the Oscillator Stop Flag (`OSF`) in Status Register
/// `0x0F`. Returns `true` if the DS3231 acknowledged the write.
pub fn write_unix_secs(unix_secs: u64) -> bool {
    if unix_secs < MIN_VALID_UNIX_SECS {
        return false;
    }
    let Some((year, month, day, wday, hour, min, sec)) = unix_to_ymd_hms(unix_secs) else {
        return false;
    };
    let Ok(guard) = I2C_BUS.try_lock() else {
        return false;
    };
    let Some(bus) = *guard else {
        return false;
    };
    drop(guard);
    let Ok(mut i2c) = bus.try_lock() else {
        return false;
    };

    let century_bit = if year >= 2100 { 0x80 } else { 0x00 };
    let yr = ((year - 2000) % 100) as u8;
    let payload = [
        REG_SECONDS,
        bin_to_bcd(sec),
        bin_to_bcd(min),
        bin_to_bcd(hour), // bit 6 = 0 selects 24-hour mode
        bin_to_bcd(wday),
        bin_to_bcd(day),
        bin_to_bcd(month) | century_bit,
        bin_to_bcd(yr),
    ];
    if i2c.blocking_write(DS3231_ADDR, &payload).is_err() {
        return false;
    }
    let mut status = [0u8; 1];
    if i2c
        .blocking_write_read(DS3231_ADDR, &[REG_STATUS], &mut status)
        .is_ok()
    {
        let cleared = status[0] & !STATUS_OSF;
        let _ = i2c.blocking_write(DS3231_ADDR, &[REG_STATUS, cleared]);
    }
    true
}
