use super::*;
use dbus::channel::{MatchingReceiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

struct Mock {
    running: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
    signals: std::sync::mpsc::Sender<dbus::Message>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Mock {
    fn start() -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (signals, incoming) = std::sync::mpsc::channel::<dbus::Message>();
        let active = running.clone();
        let recorded = calls.clone();
        let thread = std::thread::spawn(move || {
            let connection = Connection::new_session().unwrap();
            connection
                .request_name(SERVICE, false, true, false)
                .unwrap();
            connection.start_receive(
                MatchRule::new_method_call(),
                Box::new(move |message, conn| {
                    let method = message.member().unwrap().to_string();
                    recorded.lock().unwrap().push(method.clone());
                    let reply = match method.as_str() {
                        "doRegister" | "setInactive" => {
                            let (identity,) =
                                message.read1::<Vec<String>>().map(|id| (id,)).unwrap();
                            assert_eq!(identity.len(), 4);
                            message.method_return()
                        }
                        "setShortcutKeys" => {
                            let (_, keys, flags) =
                                message.read3::<Vec<String>, Keys, u32>().unwrap();
                            assert!(keys.is_empty(), "do not override user assignments");
                            assert_eq!(flags, SET_PRESENT, "autoload must stay enabled");
                            message.method_return().append1(vec![(vec![123_i32],)])
                        }
                        "getComponent" => message
                            .method_return()
                            .append1(dbus::Path::new("/component/copywraith").unwrap()),
                        _ => panic!("unexpected {method}"),
                    };
                    conn.send(reply).unwrap();
                    true
                }),
            );
            ready_tx.send(()).unwrap();
            while active.load(Ordering::SeqCst) {
                for signal in incoming.try_iter() {
                    connection.send(signal).unwrap();
                }
                connection.process(Duration::from_millis(10)).unwrap();
            }
        });
        ready_rx.recv().unwrap();
        Self {
            running,
            calls,
            thread: Some(thread),
            signals,
        }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn registers_present_actions_and_recovers_after_owner_restart() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    let expected = [
        "doRegister",
        "setShortcutKeys",
        "doRegister",
        "setShortcutKeys",
        "doRegister",
        "setShortcutKeys",
        "getComponent",
    ];
    assert_eq!(*mock.calls.lock().unwrap(), expected);
    session.poll().unwrap();
    assert_eq!(
        *mock.calls.lock().unwrap(),
        expected,
        "polling must not duplicate registration"
    );
    let old_owner = session.owner.clone();
    drop(mock);
    assert!(session.poll().is_err());
    let replacement = Mock::start();
    session.poll().unwrap();
    assert_ne!(session.owner, old_owner);
    assert_eq!(*replacement.calls.lock().unwrap(), expected);
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn rejects_forged_or_unknown_activations_and_accepts_signed_timestamp() {
    let mut session = Session::connect().unwrap();
    session.owner = ":1.42".into();
    session.component_path = "/component/copywraith".into();
    for (sender, path, component, action, expected) in [
        (
            ":1.42",
            "/component/copywraith",
            COMPONENT,
            "toggle",
            Some(Activation::Toggle),
        ),
        (":1.99", "/component/copywraith", COMPONENT, "toggle", None),
        (":1.42", "/component/other", COMPONENT, "toggle", None),
        (":1.42", "/component/copywraith", "other", "toggle", None),
        (":1.42", "/component/copywraith", COMPONENT, "delete", None),
    ] {
        let mut signal =
            dbus::Message::new_signal(path, COMPONENT_INTERFACE, "globalShortcutPressed")
                .unwrap()
                .append3(component, action, -1_i64);
        signal.set_sender(Some(dbus::strings::BusName::new(sender).unwrap()));
        assert_eq!(session.activation(&signal), expected);
    }
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn shutdown_releases_presence_without_removing_assignments() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    mock.calls.lock().unwrap().clear();
    session.deactivate().unwrap();
    assert_eq!(
        *mock.calls.lock().unwrap(),
        ["setInactive", "setInactive", "setInactive"]
    );
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn signed_activation_crosses_bus_but_forgery_does_not() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    let attacker = Connection::new_session().unwrap();
    let signal = |action| {
        dbus::Message::new_signal(
            "/component/copywraith",
            COMPONENT_INTERFACE,
            "globalShortcutPressed",
        )
        .unwrap()
        .append3(COMPONENT, action, -42_i64)
    };
    attacker.send(signal("toggle")).unwrap();
    mock.signals.send(signal("unknown-action")).unwrap();
    mock.signals.send(signal("starred")).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut received = Vec::new();
    while std::time::Instant::now() < deadline {
        received.extend(session.poll().unwrap());
        if received.contains(&Activation::Starred) {
            break;
        }
    }
    assert_eq!(received, vec![Activation::Starred]);
    assert!(session.poll().unwrap().is_empty());
}
