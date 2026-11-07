//! AppKit writes are needed because clipboard-rs clears the board again after
//! preparation, which would reset the local-host-only option for secrets.
use clipboard_rs::{common::RustImage, ClipboardContent};
use objc2::{rc::autoreleasepool, runtime::ProtocolObject};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardContentsOptions, NSPasteboardItem, NSPasteboardTypeFileURL,
    NSPasteboardTypeHTML, NSPasteboardTypePNG, NSPasteboardTypeRTF, NSPasteboardTypeString,
};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};

use super::{ClipboardSensitivity, Result};

pub(super) fn change_count() -> isize {
    NSPasteboard::generalPasteboard().changeCount()
}

pub(super) fn write(
    contents: &[ClipboardContent],
    sensitivity: ClipboardSensitivity,
) -> Result<()> {
    autoreleasepool(|_| {
        let mut items = Vec::new();
        let item = NSPasteboardItem::new();
        let mut files = false;
        for content in contents {
            let written = match content {
                ClipboardContent::Text(text) => item
                    .setString_forType(&NSString::from_str(text), unsafe {
                        NSPasteboardTypeString
                    }),
                ClipboardContent::Html(text) => item
                    .setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeHTML }),
                ClipboardContent::Rtf(text) => item
                    .setData_forType(&NSData::with_bytes(text.as_bytes()), unsafe {
                        NSPasteboardTypeRTF
                    }),
                ClipboardContent::Image(image) => {
                    let png = image.to_png().map_err(|e| e.to_string())?;
                    item.setData_forType(&NSData::with_bytes(png.get_bytes()), unsafe {
                        NSPasteboardTypePNG
                    })
                }
                ClipboardContent::Files(paths) => {
                    files = true;
                    for path in paths {
                        let path = path.strip_prefix("file://").unwrap_or(path);
                        if !std::path::Path::new(path).exists() {
                            return Err("Clipboard file no longer exists".into());
                        }
                        let file = NSPasteboardItem::new();
                        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                        let url_string =
                            url.absoluteString().ok_or("Invalid clipboard file URL")?;
                        if !file.setString_forType(&url_string, unsafe { NSPasteboardTypeFileURL })
                        {
                            return Err("Could not prepare clipboard file".into());
                        }
                        items.push(file);
                    }
                    true
                }
                ClipboardContent::Other(format, bytes) => {
                    item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(format))
                }
            };
            if !written {
                return Err("Could not prepare clipboard representation".into());
            }
        }
        if files {
            // Each URL is a separate native item. Attach metadata to every item
            // so consumers looking at just one still see its confidentiality.
            for file in &items {
                for content in contents {
                    if let ClipboardContent::Other(format, bytes) = content {
                        if !file.setData_forType(
                            &NSData::with_bytes(bytes),
                            &NSString::from_str(format),
                        ) {
                            return Err("Could not prepare clipboard file metadata".into());
                        }
                    }
                }
            }
        } else {
            items.push(item);
        }
        for item in &items {
            // History stores app names, not bundle IDs. The convention asks for
            // an empty source when the original bundle identifier is unknown.
            if !item.setString_forType(
                &NSString::from_str(""),
                &NSString::from_str("org.nspasteboard.source"),
            ) {
                return Err("Could not prepare clipboard source".into());
            }
            if sensitivity == ClipboardSensitivity::Sensitive
                && !item.setData_forType(
                    &NSData::with_bytes(&[]),
                    &NSString::from_str("org.nspasteboard.ConcealedType"),
                )
            {
                return Err("Could not prepare clipboard confidentiality marker".into());
            }
        }
        // Finish preparing every representation and marker before publishing.
        let objects = NSArray::from_retained_slice(
            &items
                .into_iter()
                .map(ProtocolObject::from_retained)
                .collect::<Vec<_>>(),
        );
        let pasteboard = NSPasteboard::generalPasteboard();
        let options = match sensitivity {
            ClipboardSensitivity::Sensitive => NSPasteboardContentsOptions::CurrentHostOnly,
            ClipboardSensitivity::Ordinary => NSPasteboardContentsOptions::empty(),
        };
        pasteboard.prepareForNewContentsWithOptions(options);
        if !pasteboard.writeObjects(&objects) {
            return Err("Could not publish clipboard contents".into());
        }
        Ok(())
    })
}
