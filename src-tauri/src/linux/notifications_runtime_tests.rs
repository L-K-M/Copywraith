use super::*;
use std::process::{Child, Command};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires isolated Xvfb, dunst and xterm"]
fn plasma_runtime_native_guidance_keeps_target_focus() {
    assert_eq!(
        std::env::var("COPYWRAITH_KDE_TEST_ISOLATED").as_deref(),
        Ok("1")
    );
    let _notifications = Process(Command::new("dunst").spawn().unwrap());
    let _target = Process(
        Command::new("xterm")
            .args(["-title", "Copywraith paste target"])
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let target = loop {
        let output = Command::new("xdotool")
            .args([
                "search",
                "--onlyvisible",
                "--name",
                "Copywraith paste target",
            ])
            .output()
            .unwrap();
        if output.status.success() {
            break String::from_utf8(output.stdout).unwrap().trim().to_string();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "paste target did not map"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(Command::new("xdotool")
        .args(["windowfocus", "--sync", &target])
        .status()
        .unwrap()
        .success());
    loop {
        if manual_paste().is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "notification daemon did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    loop {
        if Command::new("xdotool")
            .args(["search", "--onlyvisible", "--class", "Dunst"])
            .output()
            .unwrap()
            .status
            .success()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native guidance was not visible"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let focus = Command::new("xdotool")
        .arg("getwindowfocus")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(focus.stdout).unwrap().trim(), target);
}
