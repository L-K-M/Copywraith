#![cfg(not(target_os = "android"))]

#[path = "../../src-tauri/src/native_clipboard.rs"]
mod native_clipboard;

use std::sync::{mpsc, Arc};
use std::time::Duration;

use clipboard_rs::{
    common::RustImage, Clipboard, ClipboardContent, ClipboardContext, RustImageData,
};
use copywraith_core::models::ClipboardFlavors;
use native_clipboard::{ClipboardPayload, MonitorStatus, NativeClipboard};

static NATIVE_CLIPBOARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
// Upstream exposes no subscription-ready notification. This is a startup grace
// period, not clipboard polling; each assertion below waits for a native event.
const WATCHER_STARTUP: Duration = Duration::from_millis(300);
const PNG: &[u8] = include_bytes!("../../src-tauri/icons/32x32.png");

#[test]
#[ignore = "requires an isolated native clipboard (Xvfb on Linux)"]
fn native_clipboard_roundtrip_events_and_cleanup() {
    let _clipboard_guard = NATIVE_CLIPBOARD_TEST_LOCK.lock().unwrap();
    let adapter = Arc::new(NativeClipboard::new().unwrap());
    let peer = ClipboardContext::new().unwrap();
    peer.set_text("seed pasteboard change count".into())
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    let weak = Arc::downgrade(&adapter);
    adapter
        .start_monitor(
            move || {
                // Reading inside the callback verifies that no context lock is held.
                if let Some(adapter) = weak.upgrade() {
                    let _ = sender.send(adapter.read());
                }
            },
            |error| panic!("{error}"),
        )
        .unwrap();
    assert_eq!(adapter.monitor_status(), MonitorStatus::Watching);
    assert!(adapter.start_monitor(|| {}, |_| {}).is_err());
    std::thread::sleep(WATCHER_STARTUP);

    peer.set_text("external plain".into()).unwrap();
    expect_flavors(&receiver, "external plain", None, None);
    adapter.write_text("app plain").unwrap();
    expect_flavors(&receiver, "app plain", None, None);
    assert_eq!(peer.get_text().unwrap(), "app plain");

    let html = "<b>rich text</b>";
    let rtf = "{\\rtf1 rich text}";
    peer.set(vec![
        ClipboardContent::Text("rich text".into()),
        ClipboardContent::Html(html.into()),
        ClipboardContent::Rtf(rtf.into()),
    ])
    .unwrap();
    expect_flavors(&receiver, "rich text", Some(html), Some(rtf));
    adapter
        .write_flavors(&ClipboardFlavors {
            text_plain: Some("app rich".into()),
            text_html: Some(html.into()),
            text_rtf: Some(rtf.into()),
            file_list: None,
        })
        .unwrap();
    expect_flavors(&receiver, "app rich", Some(html), Some(rtf));
    assert_eq!(peer.get_html().unwrap(), html);
    assert_eq!(peer.get_rich_text().unwrap(), rtf);
    assert_eq!(peer.get_text().unwrap(), "app rich");

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("clipboard file.txt");
    std::fs::write(&file, "file payload").unwrap();
    let path = file.to_string_lossy().into_owned();
    adapter.write_files(std::slice::from_ref(&path)).unwrap();
    expect_files(&receiver, &path);
    let native_files = peer.get_files().unwrap();
    peer.set_files(native_files).unwrap();
    expect_files(&receiver, &path);

    adapter.write_image(PNG).unwrap();
    expect_image(&receiver);
    let image = peer.get_image().unwrap();
    assert_pixels(&image);
    peer.set_image(RustImageData::from_bytes(PNG).unwrap())
        .unwrap();
    expect_image(&receiver);
    assert!(adapter.write_image(b"not an image").is_err());
    assert!(adapter.write_flavors(&ClipboardFlavors::default()).is_err());
    assert!(adapter.write_files(&[]).is_err());

    adapter.stop_monitor().unwrap();
    assert_eq!(adapter.monitor_status(), MonitorStatus::Stopped);
    adapter.stop_monitor().unwrap();
    while receiver.try_recv().is_ok() {}
    peer.set_text("after stop".into()).unwrap();
    assert!(matches!(
        receiver.recv_timeout(EVENT_TIMEOUT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));

    let (sender, receiver) = mpsc::channel();
    adapter
        .start_monitor(
            move || {
                let _ = sender.send(());
            },
            |error| panic!("{error}"),
        )
        .unwrap();
    std::thread::sleep(WATCHER_STARTUP);
    peer.set_text("after restart".into()).unwrap();
    receiver.recv_timeout(EVENT_TIMEOUT).unwrap();
    drop(adapter);
    while receiver.try_recv().is_ok() {}
    assert!(matches!(
        receiver.recv_timeout(EVENT_TIMEOUT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
    // A failing callback must expose failure and still permit deterministic stop.
    let adapter = NativeClipboard::new().unwrap();
    let (sender, receiver) = mpsc::channel();
    adapter
        .start_monitor(
            || panic!("injected callback failure"),
            move |error| {
                let _ = sender.send(error);
            },
        )
        .unwrap();
    std::thread::sleep(WATCHER_STARTUP);
    peer.set_text("trigger failure".into()).unwrap();
    assert!(receiver
        .recv_timeout(EVENT_TIMEOUT)
        .unwrap()
        .contains("panicked"));
    assert!(matches!(adapter.monitor_status(), MonitorStatus::Failed(_)));
    adapter.stop_monitor().unwrap();
}

fn expect_flavors(
    receiver: &mpsc::Receiver<Result<ClipboardPayload, String>>,
    plain: &str,
    html: Option<&str>,
    rtf: Option<&str>,
) {
    let ClipboardPayload::Flavors(flavors) = receiver.recv_timeout(EVENT_TIMEOUT).unwrap().unwrap()
    else {
        panic!("expected text flavors")
    };
    assert_eq!(flavors.text_plain.as_deref(), Some(plain));
    assert_eq!(flavors.text_html.as_deref(), html);
    assert_eq!(flavors.text_rtf.as_deref(), rtf);
}

fn expect_files(receiver: &mpsc::Receiver<Result<ClipboardPayload, String>>, path: &str) {
    let ClipboardPayload::Files(files) = receiver.recv_timeout(EVENT_TIMEOUT).unwrap().unwrap()
    else {
        panic!("expected files")
    };
    assert_eq!(files, [path]);
}

fn expect_image(receiver: &mpsc::Receiver<Result<ClipboardPayload, String>>) {
    let ClipboardPayload::Image(bytes) = receiver.recv_timeout(EVENT_TIMEOUT).unwrap().unwrap()
    else {
        panic!("expected image")
    };
    assert_pixels(&RustImageData::from_bytes(&bytes).unwrap());
}

fn assert_pixels(image: &RustImageData) {
    assert_eq!(
        image.to_rgba8().unwrap(),
        RustImageData::from_bytes(PNG).unwrap().to_rgba8().unwrap()
    );
}

#[test]
fn stored_gif_remains_decodable() {
    use base64::Engine;
    // A one-pixel GIF; the removed plugin previously enabled this image codec.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")
        .unwrap();
    assert_eq!(
        RustImageData::from_bytes(&bytes).unwrap().get_size(),
        (1, 1)
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an isolated X11 clipboard (Xvfb)"]
fn unreadable_native_format_preserves_usable_payloads() {
    let _clipboard_guard = NATIVE_CLIPBOARD_TEST_LOCK.lock().unwrap();
    let adapter = NativeClipboard::new().unwrap();
    let peer = ClipboardContext::new().unwrap();
    const PNG_MIME: &str = "image/png";
    const INVALID_PNG: &[u8] = b"invalid PNG";
    const PLAIN: &str = "usable fallback";
    const HTML: &str = "<b>usable fallback</b>";
    const RTF: &str = "{\\rtf1 usable fallback}";

    // Real selection targets can advertise data that their decoder cannot read.
    peer.set(vec![
        ClipboardContent::Other(PNG_MIME.into(), INVALID_PNG.to_vec()),
        ClipboardContent::Text(PLAIN.into()),
        ClipboardContent::Html(HTML.into()),
        ClipboardContent::Rtf(RTF.into()),
    ])
    .unwrap();
    assert!(peer.has(clipboard_rs::ContentFormat::Image));
    assert!(peer.get_image().is_err());
    assert_eq!(peer.get_text().unwrap(), PLAIN);
    let ClipboardPayload::Flavors(flavors) = adapter.read().unwrap() else {
        panic!("expected usable text flavors")
    };
    assert_eq!(flavors.text_plain.as_deref(), Some(PLAIN));
    assert_eq!(flavors.text_html.as_deref(), Some(HTML));
    assert_eq!(flavors.text_rtf.as_deref(), Some(RTF));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fallback.txt");
    std::fs::write(&path, PLAIN).unwrap();
    let uri = format!("file://{}", path.display());
    peer.set(vec![
        ClipboardContent::Other(PNG_MIME.into(), INVALID_PNG.to_vec()),
        ClipboardContent::Files(vec![uri.clone()]),
        ClipboardContent::Text(PLAIN.into()),
    ])
    .unwrap();
    let ClipboardPayload::Files(files) = adapter.read().unwrap() else {
        panic!("expected file priority over text")
    };
    assert_eq!(files, [path.to_string_lossy()]);

    // X11 reports malformed file lists/empty HTML as empty, not errors.
    peer.set(vec![
        ClipboardContent::Other("text/uri-list".into(), b"not a file URI".to_vec()),
        ClipboardContent::Html(String::new()),
        ClipboardContent::Rtf(RTF.into()),
    ])
    .unwrap();
    let ClipboardPayload::Flavors(flavors) = adapter.read().unwrap() else {
        panic!("expected surviving RTF")
    };
    assert_eq!(flavors.text_html, None);
    assert_eq!(flavors.text_rtf.as_deref(), Some(RTF));

    peer.set(vec![
        ClipboardContent::Image(RustImageData::from_bytes(PNG).unwrap()),
        ClipboardContent::Files(vec![uri]),
        ClipboardContent::Text(PLAIN.into()),
    ])
    .unwrap();
    assert!(matches!(
        adapter.read().unwrap(),
        ClipboardPayload::Image(_)
    ));

    peer.set_buffer(PNG_MIME, INVALID_PNG.to_vec()).unwrap();
    assert!(
        adapter.read().is_err(),
        "all unreadable must retain an error"
    );
    peer.set_text(" ".into()).unwrap();
    assert!(matches!(adapter.read().unwrap(), ClipboardPayload::Empty));
}
