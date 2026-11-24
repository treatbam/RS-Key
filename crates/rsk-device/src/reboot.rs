// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

//! The pending-reboot state shared by the vendor applet, the worker and the trusted
//! display's ambient loop.
//!
//! Two things need to agree on this value:
//!
//! * The **worker**, which takes a queued request once the APDU / CBOR reply has
//!   flushed and then waits briefly before scrubbing and resetting — and can also
//!   enter that wait directly after a Management RESET wipe, with no request having
//!   been queued.
//! * The **display**, whose ambient loop is a synchronous busy-wait on the same
//!   thread-mode executor: once a reset is queued *or* under way, it must park
//!   itself rather than wake the panel or hold the executor during the pre-reset
//!   wait.
//!
//! A bare `swap(0)` when the worker takes the request clears the value before the
//! wait, so the next poll of the display sees "no reboot" and resumes on the single
//! `.await` point between the wipe and `sys_reset`. A fourth state (`RESETTING`)
//! keeps [`RebootSlot::pending`] true from the moment a reset is queued or begun
//! until the chip actually resets.

use core::sync::atomic::{AtomicU8, Ordering};

const NONE: u8 = 0;
/// Warm reboot (`SCB::sys_reset`).
pub const WARM: u8 = 1;
/// Secure reboot into the ROM BOOTSEL USB bootloader.
pub const BOOTSEL: u8 = 2;
/// Taken by the worker (or entered directly by a wipe's reset): the reset is now
/// under way and will not return, so [`RebootSlot::pending`] stays true while
/// [`RebootSlot::take`] yields `None`.
const RESETTING: u8 = 3;

/// One pending-reboot slot. `const`-constructible so the firmware can hold it in a
/// `static` while tests construct independent instances.
pub struct RebootSlot(AtomicU8);

impl Default for RebootSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl RebootSlot {
    pub const fn new() -> Self {
        Self(AtomicU8::new(NONE))
    }

    /// Queue a warm (`bootsel = false`) or BOOTSEL (`bootsel = true`) reboot.
    pub fn queue(&self, bootsel: bool) {
        self.0
            .store(if bootsel { BOOTSEL } else { WARM }, Ordering::Relaxed);
    }

    /// Mark that the worker has begun a reset that no queued request preceded (e.g.
    /// the post-wipe reboot at the end of a Management RESET). [`Self::pending`]
    /// stays true from here on; [`Self::take`] continues to return `None`.
    pub fn begin_reset(&self) {
        self.0.store(RESETTING, Ordering::Relaxed);
    }

    /// Take a queued reboot request for servicing, transitioning the slot to the
    /// resetting state so [`Self::pending`] remains true across the pre-reset wait
    /// while a second `take` returns `None`.
    pub fn take(&self) -> Option<u8> {
        match self.0.load(Ordering::Relaxed) {
            m @ (WARM | BOOTSEL) => {
                self.0.store(RESETTING, Ordering::Relaxed);
                Some(m)
            }
            _ => None,
        }
    }

    /// Whether a reboot is queued *or* already under way. Once true, this never
    /// returns to `false` in the lifetime of the slot (the reset ends the process).
    pub fn pending(&self) -> bool {
        self.0.load(Ordering::Relaxed) != NONE
    }
}

#[cfg(test)]
#[path = "reboot_tests.rs"]
mod tests;
