//! Native clipboard mechanics stay here; capture and paste use domain payloads.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use clipboard_rs::{
    common::RustImage, Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler,
    ClipboardWatcher, ClipboardWatcherContext, ContentFormat, RustImageData, WatcherShutdown,
};
use copywraith_core::models::ClipboardFlavors;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
pub(crate) enum ClipboardPayload {
    Image(Vec<u8>),
    Files(Vec<String>),
    Flavors(ClipboardFlavors),
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MonitorStatus {
    Stopped,
    // Worker launched; upstream provides no subscription-ready acknowledgement.
    Watching,
    Failed(String),
}

pub(crate) struct NativeClipboard {
    context: Mutex<ClipboardContext>,
    monitor: Mutex<Option<Monitor>>,
    status: Arc<Mutex<MonitorStatus>>,
}

struct Monitor {
    stopping: Arc<AtomicBool>,
    shutdown: WatcherShutdown,
    thread: JoinHandle<()>,
}

struct Handler<F>(F);

impl<F: FnMut() + Send> ClipboardHandler for Handler<F> {
    fn on_clipboard_change(&mut self) {
        (self.0)();
    }
}

impl NativeClipboard {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            context: Mutex::new(ClipboardContext::new().map_err(|e| e.to_string())?),
            monitor: Mutex::new(None),
            status: Arc::new(Mutex::new(MonitorStatus::Stopped)),
        })
    }

    fn context(&self) -> Result<MutexGuard<'_, ClipboardContext>> {
        self.context.lock().map_err(|e| e.to_string())
    }

    // Serialize reads against app writes and preserve image/file/text priority.
    pub(crate) fn read(&self) -> Result<ClipboardPayload> {
        let context = self.context()?;

        // Advertised formats can be unreadable; keep trying lower-priority data.
        let mut read_error = None;
        if context.has(ContentFormat::Image) {
            match context.get_image().and_then(|image| image.to_png()) {
                Ok(png) => return Ok(ClipboardPayload::Image(png.get_bytes().to_vec())),
                Err(error) => read_error = Some(error.to_string()),
            }
        }
        if context.has(ContentFormat::Files) {
            match context.get_files() {
                Ok(files) if !files.is_empty() => {
                    return Ok(ClipboardPayload::Files(
                        files
                            .into_iter()
                            .map(|path| path.strip_prefix("file://").unwrap_or(&path).to_string())
                            .collect(),
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    read_error.get_or_insert_with(|| error.to_string());
                }
            }
        }
        let flavors = read_text_flavors(
            |format| {
                if !context.has(format.clone()) {
                    return Ok(String::new());
                }
                match format {
                    ContentFormat::Text => context.get_text(),
                    ContentFormat::Html => context.get_html(),
                    ContentFormat::Rtf => context.get_rich_text(),
                    _ => unreachable!("Only text formats are requested"),
                }
                .map_err(|error| error.to_string())
            },
            &mut read_error,
        );
        if !flavors.is_empty() {
            return Ok(ClipboardPayload::Flavors(flavors));
        }
        match read_error {
            Some(error) => Err(error),
            None => Ok(ClipboardPayload::Empty),
        }
    }

    // Publish all text representations together so rich writes retain plaintext.
    pub(crate) fn write_flavors(&self, flavors: &ClipboardFlavors) -> Result<()> {
        let plain = flavors
            .text_plain
            .clone()
            .or_else(|| {
                flavors
                    .text_html
                    .as_ref()
                    .map(|s| copywraith_core::content::strip_html(s))
            })
            .or_else(|| {
                flavors
                    .text_rtf
                    .as_ref()
                    .map(|s| copywraith_core::content::strip_rtf(s))
            })
            .and_then(nonempty);
        let mut contents = Vec::new();
        if let Some(text) = plain {
            contents.push(ClipboardContent::Text(text));
        }
        if let Some(html) = flavors.text_html.clone().and_then(nonempty) {
            contents.push(ClipboardContent::Html(html));
        }
        if let Some(rtf) = flavors.text_rtf.clone().and_then(nonempty) {
            contents.push(ClipboardContent::Rtf(rtf));
        }
        if contents.is_empty() {
            return Err("No text flavors to write".into());
        }
        self.context()?.set(contents).map_err(|e| e.to_string())
    }

    pub(crate) fn write_text(&self, text: &str) -> Result<()> {
        self.context()?
            .set_text(text.to_string())
            .map_err(|e| e.to_string())
    }

    pub(crate) fn write_files(&self, files: &[String]) -> Result<()> {
        if files.is_empty() {
            return Err("No files to write".into());
        }
        let files = files
            .iter()
            .map(|path| {
                #[cfg(target_os = "windows")]
                return path.strip_prefix("file://").unwrap_or(path).to_string();
                #[cfg(not(target_os = "windows"))]
                if path.starts_with("file://") {
                    path.clone()
                } else {
                    format!("file://{path}")
                }
            })
            .collect();
        self.context()?.set_files(files).map_err(|e| e.to_string())
    }

    pub(crate) fn write_image(&self, bytes: &[u8]) -> Result<()> {
        let image = RustImageData::from_bytes(bytes).map_err(|e| e.to_string())?;
        self.context()?.set_image(image).map_err(|e| e.to_string())
    }

    pub(crate) fn monitor_status(&self) -> MonitorStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    // Register the callback before starting the watcher. Callbacks may read/write
    // the clipboard, but lifecycle operations belong to the owning app thread.
    pub(crate) fn start_monitor(
        &self,
        callback: impl FnMut() + Send + 'static,
        on_error: impl FnOnce(String) + Send + 'static,
    ) -> Result<()> {
        let mut monitor = self.monitor.lock().map_err(|e| e.to_string())?;
        if monitor.is_some() {
            return Err("Clipboard monitor already started; stop before restarting".into());
        }
        let mut watcher = ClipboardWatcherContext::new().map_err(|e| {
            let message = e.to_string();
            *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
                MonitorStatus::Failed(message.clone());
            message
        })?;
        watcher.add_handler(Handler(callback));
        let shutdown = watcher.get_shutdown_channel();
        let status = self.status.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        *status.lock().unwrap_or_else(|e| e.into_inner()) = MonitorStatus::Watching;
        let thread = std::thread::Builder::new()
            .name("clipboard-monitor".into())
            .spawn(move || {
                // Upstream start_watch has no Result/readiness API and may panic on
                // backend errors. Record failure instead of silently losing capture.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    watcher.start_watch()
                }));
                if worker_stopping.load(Ordering::Acquire) && result.is_ok() {
                    *status.lock().unwrap_or_else(|e| e.into_inner()) = MonitorStatus::Stopped;
                    return;
                }
                let message = if result.is_err() {
                    "Native clipboard watcher panicked"
                } else {
                    "Native clipboard watcher stopped unexpectedly"
                }
                .to_string();
                *status.lock().unwrap_or_else(|e| e.into_inner()) =
                    MonitorStatus::Failed(message.clone());
                on_error(message);
            })
            .map_err(|e| {
                *self.status.lock().unwrap_or_else(|e| e.into_inner()) =
                    MonitorStatus::Failed(e.to_string());
                e.to_string()
            })?;
        *monitor = Some(Monitor {
            stopping,
            shutdown,
            thread,
        });
        Ok(())
    }

    pub(crate) fn stop_monitor(&self) -> Result<()> {
        let mut monitor = self.monitor.lock().map_err(|e| e.to_string())?;
        if monitor
            .as_ref()
            .is_some_and(|m| m.thread.thread().id() == std::thread::current().id())
        {
            return Err("Cannot stop clipboard monitor from its callback".into());
        }
        if let Some(Monitor {
            stopping,
            shutdown,
            thread,
        }) = monitor.take()
        {
            stopping.store(true, Ordering::Release);
            shutdown.stop();
            thread
                .join()
                .map_err(|_| "Clipboard monitor thread panicked".to_string())?;
        }
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = MonitorStatus::Stopped;
        Ok(())
    }
}

impl Drop for NativeClipboard {
    fn drop(&mut self) {
        // Always signal shutdown, even after poisoning or a last Arc released by
        // a callback. A worker cannot join itself; it will exit after returning.
        let monitor = self.monitor.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Some(Monitor {
            stopping,
            shutdown,
            thread,
        }) = monitor.take()
        {
            stopping.store(true, Ordering::Release);
            shutdown.stop();
            if thread.thread().id() != std::thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

fn nonempty(text: String) -> Option<String> {
    (!text.trim().is_empty()).then_some(text)
}

// Attempt every text representation; the caller reports errors only if none work.
fn read_text_flavors(
    mut read: impl FnMut(ContentFormat) -> Result<String>,
    read_error: &mut Option<String>,
) -> ClipboardFlavors {
    let mut flavors = ClipboardFlavors::default();
    for (format, target) in [
        (ContentFormat::Text, &mut flavors.text_plain),
        (ContentFormat::Html, &mut flavors.text_html),
        (ContentFormat::Rtf, &mut flavors.text_rtf),
    ] {
        match read(format) {
            Ok(text) => *target = nonempty(text),
            Err(error) => {
                read_error.get_or_insert(error);
            }
        }
    }
    flavors
}

#[cfg(test)]
mod read_tests {
    use super::*;

    #[test]
    fn text_read_errors_do_not_discard_other_flavors() {
        // X11 getters turn read errors into empty strings; inject errors here to
        // cover backends that return Err for an advertised text representation.
        for failed_format in [ContentFormat::Text, ContentFormat::Html, ContentFormat::Rtf] {
            let mut error = None;
            let flavors = read_text_flavors(
                |format| {
                    if std::mem::discriminant(&format) == std::mem::discriminant(&failed_format) {
                        return Err("unreadable format".into());
                    }
                    Ok("usable".into())
                },
                &mut error,
            );
            assert_eq!(error.as_deref(), Some("unreadable format"));
            assert_eq!(
                flavors.text_plain.as_deref(),
                (!matches!(failed_format, ContentFormat::Text)).then_some("usable")
            );
            assert_eq!(
                flavors.text_html.as_deref(),
                (!matches!(failed_format, ContentFormat::Html)).then_some("usable")
            );
            assert_eq!(
                flavors.text_rtf.as_deref(),
                (!matches!(failed_format, ContentFormat::Rtf)).then_some("usable")
            );
        }
    }
}
