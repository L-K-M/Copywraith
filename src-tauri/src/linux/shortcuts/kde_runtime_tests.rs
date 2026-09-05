use super::*;

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
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
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
    for (action, key) in Activation::ALL.into_iter().zip(['J', 'K', 'L']) {
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
        assert!(Command::new("xdotool")
            .args([
                "key",
                "--clearmodifiers",
                &format!("ctrl+alt+shift+{}", key.to_ascii_lowercase())
            ])
            .status()
            .unwrap()
            .success());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
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

    // Persist an intentional unassignment as well as the two remaining keys.
    proxy
        .method_call::<(), _, _, _>(
            INTERFACE,
            "setForeignShortcutKeys",
            (Activation::Toggle.identity(), Keys::new()),
        )
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));
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
    assert!(Command::new("xdotool")
        .args(["key", "ctrl+alt+shift+k"])
        .status()
        .unwrap()
        .success());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if session.poll().unwrap().contains(&Activation::Starred) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "restart did not restore activation"
        );
    }
    daemon.0.kill().unwrap();
    daemon.0.wait().unwrap();
}
