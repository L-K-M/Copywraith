use super::*;
use dbus::channel::{MatchingReceiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Copy)]
enum Fault {
    Error,
    Timeout,
}

struct Mock {
    faults: Arc<Mutex<Vec<(String, String, Fault)>>>,
    actions: Arc<Mutex<Vec<(String, String)>>>,
    running: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<String>>>,
    signals: std::sync::mpsc::Sender<dbus::Message>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Mock {
    fn start() -> Self {
        let faults = Arc::new(Mutex::new(Vec::<(String, String, Fault)>::new()));
        let actions = Arc::new(Mutex::new(Vec::new()));
        let failures = faults.clone();
        let identities = actions.clone();
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
                    if let Ok(identity) = message.read1::<Vec<String>>() {
                        let action = identity[1].clone();
                        identities
                            .lock()
                            .unwrap()
                            .push((method.clone(), action.clone()));
                        let fault = {
                            let mut failures = failures.lock().unwrap();
                            failures
                                .iter()
                                .position(|(m, a, _)| m == &method && a == &action)
                                .map(|index| failures.remove(index).2)
                        };
                        match fault {
                            Some(Fault::Timeout) => return true,
                            Some(Fault::Error) => {
                                conn.send(message.error(
                                    &dbus::strings::ErrorName::new("org.test.Failed").unwrap(),
                                    &std::ffi::CString::new("injected failure").unwrap(),
                                ))
                                .unwrap();
                                return true;
                            }
                            None => {}
                        }
                    }
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
            faults,
            actions,
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

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn partial_registration_and_timeout_drop_release_every_attempted_action() {
    for fault in [Fault::Error, Fault::Timeout] {
        let mock = Mock::start();
        mock.faults
            .lock()
            .unwrap()
            .push(("setShortcutKeys".into(), "starred".into(), fault));
        let mut session = Session::connect().unwrap();
        assert!(session.poll().is_err());
        drop(session);
        let released: Vec<_> = mock
            .actions
            .lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == "setInactive")
            .map(|(_, action)| action.clone())
            .collect();
        assert_eq!(released, ["toggle", "starred"]);
    }
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn failed_deactivation_attempts_remaining_actions_and_drop_retries_failure() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    mock.actions.lock().unwrap().clear();
    mock.faults
        .lock()
        .unwrap()
        .push(("setInactive".into(), "toggle".into(), Fault::Error));
    assert!(session.deactivate().is_err());
    let released: Vec<_> = mock
        .actions
        .lock()
        .unwrap()
        .iter()
        .map(|(_, action)| action.clone())
        .collect();
    assert_eq!(released, ["toggle", "starred", "paste-plaintext"]);
    drop(session);
    assert_eq!(mock.actions.lock().unwrap().last().unwrap().1, "toggle");
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn registration_timeout_then_exit_releases_registered_actions() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    mock.faults
        .lock()
        .unwrap()
        .push(("setShortcutKeys".into(), "toggle".into(), Fault::Timeout));
    assert!(session.register(&session.owner.clone()).is_err());
    mock.actions.lock().unwrap().clear();
    drop(session);
    assert_eq!(mock.actions.lock().unwrap().len(), 3);
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn held_kf5_press_dispatches_once_until_authenticated_release() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    let signal = |member, action| {
        dbus::Message::new_signal("/component/copywraith", COMPONENT_INTERFACE, member)
            .unwrap()
            .append3(COMPONENT, action, -7_i64)
    };
    for action in Activation::ALL {
        for _ in 0..3 {
            mock.signals
                .send(signal("globalShortcutPressed", action.id()))
                .unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(800);
        let mut received = Vec::new();
        while std::time::Instant::now() < deadline {
            received.extend(session.poll().unwrap());
        }
        assert_eq!(received, [action], "held key must dispatch once");
        let attacker = Connection::new_session().unwrap();
        attacker
            .send(signal("globalShortcutReleased", action.id()))
            .unwrap();
        mock.signals
            .send(signal("globalShortcutPressed", action.id()))
            .unwrap();
        assert!(session.poll().unwrap().is_empty());
        mock.signals
            .send(signal("globalShortcutReleased", action.id()))
            .unwrap();
        mock.signals
            .send(signal("globalShortcutPressed", action.id()))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if session.poll().unwrap().contains(&action) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "release did not rearm action"
            );
        }
    }
}

#[test]
#[ignore = "requires isolated dbus-run-session"]
fn hung_daemon_cleanup_and_drop_finish_within_one_second() {
    let mock = Mock::start();
    let mut session = Session::connect().unwrap();
    session.poll().unwrap();
    for _ in 0..2 {
        for action in Activation::ALL {
            mock.faults.lock().unwrap().push((
                "setInactive".into(),
                action.id().into(),
                Fault::Timeout,
            ));
        }
    }
    let start = std::time::Instant::now();
    assert!(session.deactivate().is_err());
    drop(session);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "hung cleanup took {elapsed:?}"
    );
}
