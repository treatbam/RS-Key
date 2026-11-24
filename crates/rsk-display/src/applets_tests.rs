// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (C) 2026 RS-Key contributors

use super::*;
use crate::tests::{Env, Pad, center, nowhere};

fn view(nick: &[u8]) -> ServiceView {
    ServiceView {
        accts: [AccountRow::default(); rsk_ui::PK_ROWS_MAX],
        fids: [0u16; rsk_ui::PK_ROWS_MAX],
        page: 0,
        n: 0,
        total: 0,
        nick: Label::clamp(nick),
    }
}

#[test]
fn a_nickname_stands_in_for_the_relying_party_id() {
    let rp = Label::clamp(b"login.example.com");
    assert_eq!(service_title(&rp, &view(b"Work")).as_str(), "Work");
}

#[test]
fn without_a_nickname_the_real_relying_party_id_is_shown() {
    // The fallback is the *real* id, never a blank title — the screen's whole claim
    // is that it names who is asking.
    let rp = Label::clamp(b"login.example.com");
    assert_eq!(service_title(&rp, &view(b"")).as_str(), "login.example.com");
}

#[test]
fn a_list_poll_reports_the_row_that_was_tapped() {
    let env = Env::new();
    let rows = rsk_ui::PK_ROWS_MAX as u16;
    let target = rows - 1;
    let tap = center(rsk_ui::row_rect(rsk_ui::PK_LIST_TOP, target));
    let mut ui = env.ui(Pad::taps(&[tap]));
    assert!(matches!(ui.pick_row(rsk_ui::PK_LIST_TOP, rows), Pick::Row(i) if i == target));
}

#[test]
fn a_list_poll_reports_the_back_chevron() {
    let env = Env::new();
    let mut ui = env.ui(Pad::taps(&[center(rsk_ui::TITLE_BACK_RECT)]));
    assert!(matches!(
        ui.pick_row(rsk_ui::PK_LIST_TOP, rsk_ui::PK_ROWS_MAX as u16),
        Pick::Back
    ));
}

#[test]
fn a_queued_host_command_closes_an_open_list() {
    // A browse modal parks the worker (one thread executor), so an open list must
    // yield the moment a host command arrives rather than make it wait out the
    // inactivity bound.
    let env = Env::new();
    let mut ui = env.ui(Pad::taps(&[nowhere()]));
    ui.hooks.host_pending = true;
    assert!(matches!(
        ui.pick_row(rsk_ui::PK_LIST_TOP, rsk_ui::PK_ROWS_MAX as u16),
        Pick::Leave
    ));
}

#[test]
fn the_power_button_closes_an_open_list() {
    let env = Env::new();
    let mut ui = env.ui(Pad::idle());
    ui.hooks.press_wake(1);
    assert!(matches!(
        ui.pick_row(rsk_ui::PK_LIST_TOP, rsk_ui::PK_ROWS_MAX as u16),
        Pick::Leave
    ));
    assert!(ui.asleep, "and the panel is blanked on the way out");
}

#[test]
fn a_tap_past_the_last_loaded_row_selects_nothing() {
    // The row count is the *loaded* one, not the geometry's — a page with two rows
    // must not hand row 4 to a delete.
    let env = Env::new();
    let loaded = 2u16;
    let tap = center(rsk_ui::row_rect(rsk_ui::PK_LIST_TOP, loaded));
    let mut ui = env.ui(Pad::taps(&[tap]));
    ui.hooks.host_pending = true; // so the poll ends instead of idling out
    assert!(matches!(
        ui.pick_row(rsk_ui::PK_LIST_TOP, loaded),
        Pick::Leave
    ));
}

#[test]
fn the_cardholder_reader_can_always_see_a_truncation() {
    // `Label::clamp` can only set `truncated` if it is handed one byte more than it
    // keeps; equal buffers would let a cut value read as complete (audit run-34 #39).
    // The crate holds the rule because only it sees both constants — assert it runs.
    const _: () = assert!(rsk_openpgp::info::CH_FIELD_MAX == rsk_ui::LABEL_MAX + 1);
    let long = [b'x'; rsk_ui::LABEL_MAX + 1];
    assert!(Label::clamp(&long).truncated);
}

#[test]
fn load_oath_cred_computes_live_totp_when_rtc_is_set_and_gates_touch_creds() {
    let env = Env::new();
    let mut ui = env.ui(Pad::idle());

    // Store an 8-digit SHA-1 TOTP credential (RFC 6238 vector at t = 59 -> 94287082).
    // TLV format: 0x71 len name, 0x73 len (type|algo, digits, secret), optional 0x78 0x01 0x02 (touch).
    let name = b"GitHub:alice";
    let secret = b"12345678901234567890";
    let mut rec = Vec::new();
    rec.push(0x71);
    rec.push(name.len() as u8);
    rec.extend_from_slice(name);
    rec.push(0x73);
    rec.push((2 + secret.len()) as u8);
    rec.push(0x21); // TOTP | SHA-1
    rec.push(8); // 8 digits
    rec.extend_from_slice(secret);
    env.fs
        .borrow_mut()
        .put(0xBA00, &rec)
        .expect("write OATH cred");
    {
        let mkek = read_fused(ui.keys.mkek_source);
        let dev = ui.keys.device(&mkek);
        let mut rng = env.rng.borrow_mut();
        let mut fs = env.fs.borrow_mut();
        rsk_oath::migrate_seal(&dev, &mut fs, &mut *rng);
    }

    // 1. Without RTC clock set -> clock_set = false, code = None.
    ui.hooks.rtc_secs = None;
    let v_unset = ui.load_oath_cred(0, false);
    assert!(!v_unset.clock_set);
    assert_eq!(v_unset.code, None);

    // 2. With DS3231 RTC at t = 59 -> live 8-digit TOTP code 94287082, 1s remaining.
    ui.hooks.rtc_secs = Some(59);
    let v_live = ui.load_oath_cred(0, false);
    assert!(v_live.clock_set);
    assert_eq!(v_live.code, Some(94_287_082));
    assert_eq!(v_live.remaining_secs, 1);

    // 3. When PROP_TOUCH (tag 0x78, len 1, val 0x02) is set, code stays hidden until revealed = true.
    rec.extend_from_slice(&[0x78, 0x01, 0x02]);
    env.fs
        .borrow_mut()
        .put(0xBA00, &rec)
        .expect("write touch OATH cred");
    {
        let mkek = read_fused(ui.keys.mkek_source);
        let dev = ui.keys.device(&mkek);
        let mut rng = env.rng.borrow_mut();
        let mut fs = env.fs.borrow_mut();
        rsk_oath::migrate_seal(&dev, &mut fs, &mut *rng);
    }
    let v_hidden = ui.load_oath_cred(0, false);
    assert!(v_hidden.touch);
    assert!(v_hidden.clock_set);
    assert_eq!(v_hidden.code, None);

    let v_revealed = ui.load_oath_cred(0, true);
    assert_eq!(v_revealed.code, Some(94_287_082));
}
