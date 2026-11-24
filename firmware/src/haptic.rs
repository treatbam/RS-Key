// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! Optional non-blocking haptic feedback (`none`, `pwm`, or `drv2605`) with
//! runtime intensity, PIN-pad click mute, and a two-stage overdrive + sustain
//! PWM envelope tailored to phone ERM motors.
//!
//! Callers fire [`pulse_touch`], [`pin_prompt`], [`pin_error`], and
//! [`test_cues`] from display and protocol handlers without blocking. When a
//! haptic backend is enabled, an async task on `EXECUTOR_HIGH` consumes the
//! highest-priority pending cue so touch sampling, synchronous UI loops, and
//! constant-time crypto paths are never delayed.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
use embassy_time::{Duration, Timer};
#[cfg(feature = "display")]
use rsk_ui::{HAPTIC_LEVEL_DEFAULT, HAPTIC_LEVEL_HIGH};
#[cfg(not(feature = "display"))]
const HAPTIC_LEVEL_DEFAULT: u8 = 2;
#[cfg(not(feature = "display"))]
const HAPTIC_LEVEL_HIGH: u8 = 3;
#[cfg(all(feature = "display", any(haptic_kind = "pwm", haptic_kind = "drv2605")))]
use rsk_ui::{HAPTIC_LEVEL_LOW, HAPTIC_LEVEL_OFF};
#[cfg(all(
    not(feature = "display"),
    any(haptic_kind = "pwm", haptic_kind = "drv2605")
))]
const HAPTIC_LEVEL_OFF: u8 = 0;
#[cfg(all(
    not(feature = "display"),
    any(haptic_kind = "pwm", haptic_kind = "drv2605")
))]
const HAPTIC_LEVEL_LOW: u8 = 1;

#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
const EVENT_NONE: u8 = 0;
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
const EVENT_TOUCH: u8 = 1;
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
const EVENT_PIN_PROMPT: u8 = 2;
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
const EVENT_PIN_ERROR: u8 = 3;
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
const EVENT_TEST_CUES: u8 = 4;

#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
static BUSY: AtomicBool = AtomicBool::new(false);
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
static PENDING: AtomicU8 = AtomicU8::new(EVENT_NONE);
#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Runtime haptic intensity level (`0` = Off, `1` = Low, `2` = Medium, `3` = High).
static LEVEL: AtomicU8 = AtomicU8::new(HAPTIC_LEVEL_DEFAULT);
/// Whether touch clicks are muted while the on-screen PIN pad is open (acoustic
/// side-channel privacy).
static MUTE_PIN: AtomicBool = AtomicBool::new(false);
/// Set while `rsk_display` is inside `collect_pin` so [`pulse_touch`] can suppress
/// per-key clicks when [`MUTE_PIN`] is enabled.
static PIN_PAD_OPEN: AtomicBool = AtomicBool::new(false);

/// Read the active runtime haptic intensity level (`0..=3`).
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed)
}

/// Set the active runtime haptic intensity level (`0..=3`, clamped).
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn set_level(lvl: u8) {
    LEVEL.store(lvl.min(HAPTIC_LEVEL_HIGH), Ordering::Relaxed);
}

/// Read whether PIN-pad touch clicks are muted.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn mute_pin() -> bool {
    MUTE_PIN.load(Ordering::Relaxed)
}

/// Set whether PIN-pad touch clicks are muted.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn set_mute_pin(mute: bool) {
    MUTE_PIN.store(mute, Ordering::Relaxed);
}

/// Mark whether the on-screen PIN pad is currently open.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn set_pin_pad_open(open: bool) {
    PIN_PAD_OPEN.store(open, Ordering::Relaxed);
}

/// Fire a short sharp pulse (~15–24 ms) on a registered touch contact.
///
/// Dropped immediately if haptics are turned off, if the PIN pad is open with
/// PIN clicks muted, or if the actuator is currently playing a pattern or has
/// another cue queued, so rapid touch polling never queues up delayed clicks.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn pulse_touch() {
    #[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
    {
        if LEVEL.load(Ordering::Relaxed) == HAPTIC_LEVEL_OFF {
            return;
        }
        if PIN_PAD_OPEN.load(Ordering::Relaxed) && MUTE_PIN.load(Ordering::Relaxed) {
            return;
        }
        if !BUSY.load(Ordering::Relaxed)
            && PENDING
                .compare_exchange(
                    EVENT_NONE,
                    EVENT_TOUCH,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
        {
            WAKE.signal(());
        }
    }
}

/// Fire a dull double-pulse cue when the user is prompted to enter a PIN.
///
/// Coalesces with any existing queued prompt and upgrades a queued touch pulse
/// so the prompt cue is not lost right after a screen transition.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn pin_prompt() {
    #[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
    {
        if LEVEL.load(Ordering::Relaxed) == HAPTIC_LEVEL_OFF {
            return;
        }
        let mut cur = PENDING.load(Ordering::Relaxed);
        while cur < EVENT_PIN_PROMPT {
            match PENDING.compare_exchange_weak(
                cur,
                EVENT_PIN_PROMPT,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    WAKE.signal(());
                    break;
                }
                Err(actual) => cur = actual,
            }
        }
    }
}

/// Fire one long buzz (~450 ms) when a PIN is rejected or blocked.
///
/// Supersedes any pending touch or prompt cue so tapping the PIN pad's `OK`
/// button never swallows the subsequent wrong-PIN buzz.
#[inline]
pub fn pin_error() {
    #[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
    {
        if LEVEL.load(Ordering::Relaxed) == HAPTIC_LEVEL_OFF {
            return;
        }
        PENDING.store(EVENT_PIN_ERROR, Ordering::Relaxed);
        WAKE.signal(());
    }
}

/// Play a demonstration sequence of all three cues (touch click, dull double
/// prompt, and rejection buzz) from `Settings → Display → Haptics → Test cues`.
#[cfg_attr(not(feature = "display"), allow(dead_code))]
#[inline]
pub fn test_cues() {
    #[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
    {
        if LEVEL.load(Ordering::Relaxed) == HAPTIC_LEVEL_OFF {
            return;
        }
        PENDING.store(EVENT_TEST_CUES, Ordering::Relaxed);
        WAKE.signal(());
    }
}

#[cfg(any(haptic_kind = "pwm", haptic_kind = "drv2605"))]
async fn next_event() -> u8 {
    loop {
        let ev = PENDING.swap(EVENT_NONE, Ordering::Acquire);
        if ev != EVENT_NONE {
            return ev;
        }
        WAKE.wait().await;
    }
}

// ---------------------------------------------------------------------------
// PWM ERM backend (`kind = "pwm"`)
// ---------------------------------------------------------------------------

#[cfg(haptic_kind = "pwm")]
use embassy_rp::pwm::{Config as PwmConfig, Pwm};

#[cfg(haptic_kind = "pwm")]
const PWM_TOP: u16 = 255;
#[cfg(haptic_kind = "pwm")]
const PWM_DUTY_OVERDRIVE: u16 = 255;
#[cfg(haptic_kind = "pwm")]
const PWM_PROMPT_KICK_DUTY: u16 = 220;
#[cfg(haptic_kind = "pwm")]
const PWM_PROMPT_KICK_MS: u64 = 8;
#[cfg(haptic_kind = "pwm")]
const PWM_PROMPT_SUSTAIN_MS: u64 = 28;
#[cfg(haptic_kind = "pwm")]
const PWM_PROMPT_GAP_MS: u64 = 70;
#[cfg(haptic_kind = "pwm")]
const PWM_ERROR_KICK_MS: u64 = 35;
#[cfg(haptic_kind = "pwm")]
const PWM_ERROR_SUSTAIN_MS: u64 = 415;

#[cfg(haptic_kind = "pwm")]
#[inline]
fn set_pwm_duty(pwm: &mut Pwm<'static>, cfg: &mut PwmConfig, channel: u8, duty: u16) {
    if channel == 0 {
        cfg.compare_a = duty;
    } else {
        cfg.compare_b = duty;
    }
    pwm.set_config(cfg);
}

/// Play a single PWM haptic cue using a two-stage overdrive + sustain envelope
/// tuned for the rotor inertia of a phone ERM motor.
#[cfg(haptic_kind = "pwm")]
async fn pwm_play_one(pwm: &mut Pwm<'static>, cfg: &mut PwmConfig, channel: u8, ev: u8, lvl: u8) {
    let (touch_ms, prompt_sustain_duty, error_sustain_duty) = match lvl {
        HAPTIC_LEVEL_LOW => (14u64, 78u16, 185u16),
        HAPTIC_LEVEL_HIGH => (24u64, 118u16, 255u16),
        _ => (19u64, 96u16, 220u16),
    };
    match ev {
        EVENT_TOUCH => {
            set_pwm_duty(pwm, cfg, channel, PWM_DUTY_OVERDRIVE);
            Timer::after(Duration::from_millis(touch_ms)).await;
            set_pwm_duty(pwm, cfg, channel, 0);
        }
        EVENT_PIN_PROMPT => {
            for i in 0..2 {
                if i > 0 {
                    Timer::after(Duration::from_millis(PWM_PROMPT_GAP_MS)).await;
                }
                // Short overdrive kick breaks static friction so the eccentric
                // mass starts turning before dropping to the dull sustain duty.
                set_pwm_duty(pwm, cfg, channel, PWM_PROMPT_KICK_DUTY);
                Timer::after(Duration::from_millis(PWM_PROMPT_KICK_MS)).await;
                set_pwm_duty(pwm, cfg, channel, prompt_sustain_duty);
                Timer::after(Duration::from_millis(PWM_PROMPT_SUSTAIN_MS)).await;
                set_pwm_duty(pwm, cfg, channel, 0);
            }
        }
        EVENT_PIN_ERROR => {
            set_pwm_duty(pwm, cfg, channel, PWM_DUTY_OVERDRIVE);
            Timer::after(Duration::from_millis(PWM_ERROR_KICK_MS)).await;
            set_pwm_duty(pwm, cfg, channel, error_sustain_duty);
            Timer::after(Duration::from_millis(PWM_ERROR_SUSTAIN_MS)).await;
            set_pwm_duty(pwm, cfg, channel, 0);
        }
        _ => {}
    }
}

#[cfg(haptic_kind = "pwm")]
#[embassy_executor::task]
pub async fn pwm_task(mut pwm: Pwm<'static>, channel: u8) {
    // 160 MHz / 32 / (255 + 1) = 19.53 kHz ultrasonic PWM carrier.
    let mut cfg = PwmConfig::default();
    cfg.top = PWM_TOP;
    cfg.divider = 32u8.into();
    cfg.compare_a = 0;
    cfg.compare_b = 0;
    pwm.set_config(&cfg);

    loop {
        let ev = next_event().await;
        let lvl = LEVEL.load(Ordering::Relaxed);
        if lvl == HAPTIC_LEVEL_OFF {
            continue;
        }
        BUSY.store(true, Ordering::Release);
        if ev == EVENT_TEST_CUES {
            pwm_play_one(&mut pwm, &mut cfg, channel, EVENT_TOUCH, lvl).await;
            Timer::after(Duration::from_millis(220)).await;
            pwm_play_one(&mut pwm, &mut cfg, channel, EVENT_PIN_PROMPT, lvl).await;
            Timer::after(Duration::from_millis(260)).await;
            pwm_play_one(&mut pwm, &mut cfg, channel, EVENT_PIN_ERROR, lvl).await;
        } else {
            pwm_play_one(&mut pwm, &mut cfg, channel, ev, lvl).await;
        }
        BUSY.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// TI DRV2605L I2C backend (`kind = "drv2605"`)
// ---------------------------------------------------------------------------

#[cfg(haptic_kind = "drv2605")]
const DRV2605_ADDR: u8 = 0x5A;
#[cfg(haptic_kind = "drv2605")]
const REG_MODE: u8 = 0x01;
#[cfg(haptic_kind = "drv2605")]
const REG_RTP_INPUT: u8 = 0x02;
#[cfg(haptic_kind = "drv2605")]
const REG_LIBRARY: u8 = 0x03;
#[cfg(haptic_kind = "drv2605")]
const REG_WAVESEQ1: u8 = 0x04;
#[cfg(haptic_kind = "drv2605")]
const REG_GO: u8 = 0x0C;
#[cfg(haptic_kind = "drv2605")]
const REG_OVERDRIVE: u8 = 0x0D;
#[cfg(haptic_kind = "drv2605")]
const REG_SUSTAIN_POS: u8 = 0x0E;
#[cfg(haptic_kind = "drv2605")]
const REG_SUSTAIN_NEG: u8 = 0x0F;
#[cfg(haptic_kind = "drv2605")]
const REG_BREAK: u8 = 0x10;
#[cfg(haptic_kind = "drv2605")]
const REG_FEEDBACK: u8 = 0x1A;

// Built-in ROM waveform IDs (TI DRV2605L datasheet Table 27):
//   1   = Strong Click - 100% (~15-20 ms)
//   2   = Strong Click - 60%
//   3   = Strong Click - 30%
//   7   = Soft Bump - 100%
//   8   = Soft Bump - 60%
//   9   = Soft Bump - 30%
//   118 = Long buzz for programmatic stopping - 100% (holds until GO is cleared)
//   0x86 = Wait 60 ms (bit 7 = wait flag, bits 6..0 = 6 * 10 ms)
#[cfg(haptic_kind = "drv2605")]
const EFFECT_STRONG_CLICK_100: u8 = 1;
#[cfg(haptic_kind = "drv2605")]
const EFFECT_STRONG_CLICK_60: u8 = 2;
#[cfg(haptic_kind = "drv2605")]
const EFFECT_STRONG_CLICK_30: u8 = 3;
#[cfg(haptic_kind = "drv2605")]
const EFFECT_SOFT_BUMP_100: u8 = 7;
#[cfg(haptic_kind = "drv2605")]
const EFFECT_SOFT_BUMP_30: u8 = 9;
#[cfg(haptic_kind = "drv2605")]
const EFFECT_LONG_BUZZ_100: u8 = 118;
#[cfg(haptic_kind = "drv2605")]
const WAIT_60_MS: u8 = 0x80 | 6;

#[cfg(all(haptic_kind = "drv2605", haptic_actuator = "erm"))]
const DRV2605_FEEDBACK_VAL: u8 = 0x36; // bit 7 = 0 (ERM), FB_BRAKE=3, LOOP_GAIN=1, BEMF_GAIN=2
#[cfg(all(haptic_kind = "drv2605", haptic_actuator = "erm"))]
const DRV2605_LIBRARY_VAL: u8 = 0x01; // Library A (ERM open/closed-loop)

#[cfg(all(haptic_kind = "drv2605", haptic_actuator = "lra"))]
const DRV2605_FEEDBACK_VAL: u8 = 0xB6; // bit 7 = 1 (LRA), FB_BRAKE=3, LOOP_GAIN=1, BEMF_GAIN=2
#[cfg(all(haptic_kind = "drv2605", haptic_actuator = "lra"))]
const DRV2605_LIBRARY_VAL: u8 = 0x06; // Library F (LRA)

#[cfg(haptic_kind = "drv2605")]
async fn drv2605_init(bus: &'static crate::rtc::SharedI2c1) -> bool {
    let mut i2c = bus.lock().await;
    let init_regs: &[(u8, u8)] = &[
        (REG_MODE, 0x00), // Exit standby, internal trigger mode
        (REG_RTP_INPUT, 0x00),
        (REG_OVERDRIVE, 0x00),
        (REG_SUSTAIN_POS, 0x00),
        (REG_SUSTAIN_NEG, 0x00),
        (REG_BREAK, 0x00),
        (REG_FEEDBACK, DRV2605_FEEDBACK_VAL),
        (REG_LIBRARY, DRV2605_LIBRARY_VAL),
    ];
    for &(reg, val) in init_regs {
        if i2c.blocking_write(DRV2605_ADDR, &[reg, val]).is_err() {
            return false;
        }
    }
    true
}

#[cfg(haptic_kind = "drv2605")]
async fn drv2605_play(
    bus: &'static crate::rtc::SharedI2c1,
    seq: &[u8; 4],
    hold_ms: u64,
    stop_after: bool,
) {
    {
        let mut i2c = bus.lock().await;
        let _ = i2c.blocking_write(DRV2605_ADDR, &[REG_GO, 0x00]);
        let _ = i2c.blocking_write(
            DRV2605_ADDR,
            &[REG_WAVESEQ1, seq[0], seq[1], seq[2], seq[3]],
        );
        let _ = i2c.blocking_write(DRV2605_ADDR, &[REG_GO, 0x01]);
    }
    Timer::after(Duration::from_millis(hold_ms)).await;
    if stop_after {
        let mut i2c = bus.lock().await;
        let _ = i2c.blocking_write(DRV2605_ADDR, &[REG_GO, 0x00]);
    }
}

#[cfg(haptic_kind = "drv2605")]
async fn drv2605_play_one(bus: &'static crate::rtc::SharedI2c1, ev: u8, lvl: u8) {
    let (click_eff, bump_eff) = match lvl {
        HAPTIC_LEVEL_LOW => (EFFECT_STRONG_CLICK_30, EFFECT_SOFT_BUMP_30),
        HAPTIC_LEVEL_HIGH => (EFFECT_STRONG_CLICK_100, EFFECT_SOFT_BUMP_100),
        _ => (EFFECT_STRONG_CLICK_60, EFFECT_SOFT_BUMP_30),
    };
    match ev {
        EVENT_TOUCH => {
            drv2605_play(bus, &[click_eff, 0, 0, 0], 20, false).await;
        }
        EVENT_PIN_PROMPT => {
            drv2605_play(bus, &[bump_eff, WAIT_60_MS, bump_eff, 0], 140, false).await;
        }
        EVENT_PIN_ERROR => {
            drv2605_play(bus, &[EFFECT_LONG_BUZZ_100, 0, 0, 0], 450, true).await;
        }
        _ => {}
    }
}

#[cfg(haptic_kind = "drv2605")]
#[embassy_executor::task]
pub async fn drv2605_task(bus: &'static crate::rtc::SharedI2c1) {
    let mut initialized = drv2605_init(bus).await;

    loop {
        let ev = next_event().await;
        let lvl = LEVEL.load(Ordering::Relaxed);
        if lvl == HAPTIC_LEVEL_OFF {
            continue;
        }
        BUSY.store(true, Ordering::Release);
        if !initialized {
            initialized = drv2605_init(bus).await;
        }
        if initialized {
            if ev == EVENT_TEST_CUES {
                drv2605_play_one(bus, EVENT_TOUCH, lvl).await;
                Timer::after(Duration::from_millis(220)).await;
                drv2605_play_one(bus, EVENT_PIN_PROMPT, lvl).await;
                Timer::after(Duration::from_millis(260)).await;
                drv2605_play_one(bus, EVENT_PIN_ERROR, lvl).await;
            } else {
                drv2605_play_one(bus, ev, lvl).await;
            }
        }
        BUSY.store(false, Ordering::Release);
    }
}
