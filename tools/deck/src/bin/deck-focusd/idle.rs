//! Idle and resume from the compositor, through Wayland's ext-idle-notify-v1. The notification
//! respects idle inhibitors: while a video plays with one, the user does not go idle.

use crate::Msg;
use anyhow::{Context, Result, bail};
use std::sync::mpsc::Sender;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1, ext_idle_notifier_v1};

struct State {
    tx: Sender<Msg>,
    gone: bool,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_idle_notifier_v1::ExtIdleNotifierV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ext_idle_notifier_v1::ExtIdleNotifierV1,
        _: ext_idle_notifier_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let msg = match event {
            ext_idle_notification_v1::Event::Idled => Msg::Idle(true),
            ext_idle_notification_v1::Event::Resumed => Msg::Idle(false),
            _ => return,
        };
        state.gone = state.tx.send(msg).is_err();
    }
}

/// Sends Idle(true) after `timeout_ms` without input and Idle(false) on the next input; returns
/// only on error.
pub fn watch(tx: Sender<Msg>, timeout_ms: u32) -> Result<()> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland display")?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).context("no wl_seat")?;
    let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals
        .bind(&qh, 1..=1, ())
        .context("the compositor has no ext_idle_notifier_v1")?;
    let _notification = notifier.get_idle_notification(timeout_ms, &seat, &qh, ());
    let mut state = State { tx, gone: false };
    while !state.gone {
        queue.blocking_dispatch(&mut state)?;
    }
    bail!("the receiver went away")
}
