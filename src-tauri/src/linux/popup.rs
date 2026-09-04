//! Present the GTK popup atomically so focus cannot overtake visibility.

use gtk::prelude::*;

pub(super) fn show(popup: &tauri::WebviewWindow) -> tauri::Result<()> {
    popup.with_webview(|webview| {
        let webview = webview.inner();
        let Some(window) = webview
            .toplevel()
            .and_then(|widget| widget.downcast::<gtk::Window>().ok())
        else {
            log::warn!("Could not find the Linux popup's GTK window");
            return;
        };

        // Tao queues show(), but ignores set_focus() while still hidden.
        // Perform both GTK operations on the main thread before focusing WebKit.
        window.set_keep_above(true);
        window.show_all();
        window.present();
        webview.grab_focus();
    })
}
