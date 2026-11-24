// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

use super::*;

#[test]
fn pending_holds_across_take_until_the_reset() {
    // Taking the request is the moment the worker *starts* the reset (the 200 ms
    // reply-flush wait runs between `take` and `sys_reset`, and that wait yields to
    // the display task on the same executor). `pending` must stay true across it,
    // while a second `take` must not re-fire the request.
    for (bootsel, mode) in [(false, WARM), (true, BOOTSEL)] {
        let slot = RebootSlot::new();
        assert!(!slot.pending());
        assert_eq!(slot.take(), None);

        slot.queue(bootsel);
        assert!(slot.pending());
        assert_eq!(slot.take(), Some(mode));
        assert!(
            slot.pending(),
            "pending cleared on take — the display would resume during the pre-reset wait"
        );
        assert_eq!(slot.take(), None, "a taken request must not be taken twice");
    }
}

#[test]
fn begin_reset_marks_pending_without_a_queued_request() {
    // A Management RESET over USB wipes the store and enters `Worker::reboot`
    // directly without queueing through `request_reboot`. The pre-reset wait still
    // yields to the display, so `begin_reset` must park it without making `take`
    // report a fresh request.
    let slot = RebootSlot::new();
    slot.begin_reset();
    assert!(slot.pending());
    assert_eq!(slot.take(), None);
}
