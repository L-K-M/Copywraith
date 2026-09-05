use super::*;

const TEST_BINDINGS: [(Activation, char); 3] = [
    (Activation::Toggle, 'J'),
    (Activation::Starred, 'K'),
    (Activation::Plaintext, 'L'),
];
const READY_TIMEOUT: Duration = Duration::from_secs(10);
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(5);

fn press_key(action: Activation) {
    let (_, key) = TEST_BINDINGS
        .iter()
        .find(|(candidate, _)| *candidate == action)
        .unwrap();
    assert!(std::process::Command::new("xdotool")
        .args([
            "key",
            "--clearmodifiers",
            &format!("ctrl+alt+shift+{}", key.to_ascii_lowercase())
        ])
        .status()
        .unwrap()
        .success());
}

fn persisted_key(config: &str, action: Activation) -> Option<&str> {
    let mut in_component = false;
    for line in config.lines() {
        if line.starts_with('[') {
            in_component = line == format!("[{COMPONENT}]");
            continue;
        }
        if !in_component {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name == action.id() {
            return value.split(',').next();
        }
    }
    None
}

fn wait_persisted_key(action: Activation, expected: &str) {
    let path = std::path::PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap())
        .join("kglobalshortcutsrc");
    let deadline = std::time::Instant::now() + READY_TIMEOUT;
    loop {
        let config = std::fs::read_to_string(&path).unwrap_or_default();
        if persisted_key(&config, action) == Some(expected) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{action:?} assignment was not flushed to {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn persisted_key_reads_only_the_component_and_current_assignment() {
    let config = "[other]\ntoggle=none,none,Other\n[copywraith]\ntoggle=Ctrl+Alt+Shift+J,none,Toggle popup\n[another]\ntoggle=none,none,Other\n";
    assert_eq!(
        persisted_key(config, Activation::Toggle),
        Some("Ctrl+Alt+Shift+J")
    );
    assert_eq!(persisted_key(config, Activation::Starred), None);
    assert_eq!(
        persisted_key("[other]\ntoggle=none,none,Other\n", Activation::Toggle),
        None
    );
    assert_eq!(
        persisted_key(
            "[copywraith]\ntoggle=none,none,Toggle popup\n",
            Activation::Toggle
        ),
        Some("none")
    );
}

/// Run only through scripts/test-kde-runtime.sh on a disposable Plasma session.
#[test]
#[ignore = "requires real KGlobalAccel, Xvfb and an isolated session"]
fn plasma_runtime_assignments_activation_and_restart() {
    use std::process::Command;
    assert_eq!(
        std::env::var("COPYWRAITH_KDE_TEST_ISOLATED").as_deref(),
        Ok("1")
    );
    let daemon_path = std::env::var("COPYWRAITH_KGLOBALACCELD").unwrap();
    struct Daemon(std::process::Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut daemon = Daemon(Command::new(&daemon_path).spawn().unwrap());
    let mut session = Session::connect().unwrap();
    let wait_ready = |session: &mut Session| {
        let deadline = std::time::Instant::now() + READY_TIMEOUT;
        loop {
            if session.poll().is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "KGlobalAccel did not become ready"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    wait_ready(&mut session);
    let old_owner = session.owner.clone();
    let connection = Connection::new_session().unwrap();
    let proxy = connection.with_proxy(SERVICE, ROOT, TIMEOUT);
    let (actions,): (Vec<Vec<String>>,) = proxy
        .method_call(
            INTERFACE,
            "allActionsForComponent",
            (Activation::Toggle.identity(),),
        )
        .unwrap();
    assert_eq!(
        actions.len(),
        Activation::ALL.len(),
        "actions must be visible to System Settings"
    );

    // Qt::CTRL | Qt::ALT | Qt::SHIFT, with three distinct letter keys.
    const TEST_MODIFIERS: i32 = 0x04000000 | 0x08000000 | 0x02000000;
    for (action, key) in TEST_BINDINGS {
        let assigned: Keys = vec![(vec![TEST_MODIFIERS | key as i32, 0, 0, 0],)];
        proxy
            .method_call::<(), _, _, _>(
                INTERFACE,
                "setForeignShortcutKeys",
                (action.identity(), assigned.clone()),
            )
            .unwrap();
        session.register(&old_owner).unwrap();
        let (saved,): (Keys,) = proxy
            .method_call(INTERFACE, "shortcutKeys", (action.identity(),))
            .unwrap();
        assert_eq!(saved, assigned, "re-registration must preserve custom keys");
        press_key(action);
        let deadline = std::time::Instant::now() + ACTIVATION_TIMEOUT;
        loop {
            if session.poll().unwrap().contains(&action) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "real key did not activate {action:?}"
            );
        }
    }

    // Observe the assigned value first so an older empty config cannot satisfy the wait.
    let (_, toggle_key) = TEST_BINDINGS
        .iter()
        .find(|(action, _)| *action == Activation::Toggle)
        .unwrap();
    wait_persisted_key(Activation::Toggle, &format!("Ctrl+Alt+Shift+{toggle_key}"));

    // Persist an intentional unassignment as well as the two remaining keys.
    proxy
        .method_call::<(), _, _, _>(
            INTERFACE,
            "setForeignShortcutKeys",
            (Activation::Toggle.identity(), Keys::new()),
        )
        .unwrap();
    // KDE batches writes; wait for the saved value instead of a fixed grace sleep.
    wait_persisted_key(Activation::Toggle, "none");
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
    daemon = Daemon(Command::new(&daemon_path).spawn().unwrap());
    wait_ready(&mut session);
    assert_ne!(session.owner, old_owner);
    let (unbound,): (Keys,) = proxy
        .method_call(INTERFACE, "shortcutKeys", (Activation::Toggle.identity(),))
        .unwrap();
    assert!(
        unbound.is_empty(),
        "restart must preserve a disabled action"
    );
    for action in [Activation::Starred, Activation::Plaintext] {
        let (saved,): (Keys,) = proxy
            .method_call(INTERFACE, "shortcutKeys", (action.identity(),))
            .unwrap();
        assert!(!saved.is_empty(), "restart lost assignment");
    }
    // The enabled keys mark delivery after the disabled combo on the same X server.
    press_key(Activation::Toggle);
    for action in [Activation::Starred, Activation::Plaintext] {
        press_key(action);
        let deadline = std::time::Instant::now() + ACTIVATION_TIMEOUT;
        loop {
            let activations = session.poll().unwrap();
            assert!(
                !activations.contains(&Activation::Toggle),
                "disabled key activated after restart"
            );
            if activations.contains(&action) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "restart did not restore {action:?}"
            );
        }
    }
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
}
