//! Native desktop guidance, independent of popup visibility and focus.
use dbus::blocking::Connection;
use std::collections::HashMap;
use std::time::Duration;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_DEFAULT_EXPIRY: i32 = -1;
const NEW_NOTIFICATION: u32 = 0;

pub(super) fn manual_paste() -> Result<(), String> {
    let connection = Connection::new_session().map_err(|e| e.to_string())?;
    let proxy = connection.with_proxy(SERVICE, PATH, TIMEOUT);
    // Empty actions cannot activate our window; the target keeps keyboard focus.
    let hints: HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>> = HashMap::new();
    let (_id,): (u32,) = proxy
        .method_call(
            SERVICE,
            "Notify",
            (
                "Copywraith",
                NEW_NOTIFICATION,
                "copywraith",
                "Copied to clipboard",
                "Press Ctrl+V to paste. Install ydotool for automatic paste.",
                Vec::<String>::new(),
                hints,
                SERVER_DEFAULT_EXPIRY,
            ),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::channel::{MatchingReceiver, Sender};
    use dbus::message::MatchRule;
    use std::sync::mpsc;

    #[test]
    #[ignore = "requires isolated dbus-run-session"]
    fn guidance_uses_native_notification_without_focus_actions() {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (called_tx, called_rx) = mpsc::channel();
        let daemon = std::thread::spawn(move || {
            let connection = Connection::new_session().unwrap();
            connection
                .request_name(SERVICE, false, true, false)
                .unwrap();
            connection.start_receive(
                MatchRule::new_method_call(),
                Box::new(move |message, conn| {
                    assert_eq!(message.member().unwrap().to_string(), "Notify");
                    let mut args = message.iter_init();
                    assert_eq!(args.read::<String>().unwrap(), "Copywraith");
                    assert_eq!(args.read::<u32>().unwrap(), NEW_NOTIFICATION);
                    assert_eq!(args.read::<String>().unwrap(), "copywraith");
                    assert_eq!(args.read::<String>().unwrap(), "Copied to clipboard");
                    assert!(args.read::<String>().unwrap().contains("Ctrl+V"));
                    assert!(args.read::<Vec<String>>().unwrap().is_empty());
                    let _: dbus::arg::PropMap = args.read().unwrap();
                    assert_eq!(args.read::<i32>().unwrap(), SERVER_DEFAULT_EXPIRY);
                    conn.send(message.method_return().append1(1_u32)).unwrap();
                    called_tx.send(()).unwrap();
                    true
                }),
            );
            ready_tx.send(()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                connection.process(Duration::from_millis(10)).unwrap();
            }
        });
        ready_rx.recv().unwrap();
        manual_paste().unwrap();
        let received = called_rx.recv_timeout(Duration::from_secs(4));
        daemon.join().unwrap();
        received.expect("guidance must reach the native notification daemon");
    }
}

#[cfg(test)]
#[path = "notifications_runtime_tests.rs"]
mod runtime_tests;
