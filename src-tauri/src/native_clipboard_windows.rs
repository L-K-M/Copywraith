//! Keep one Windows clipboard handle open while publishing the payload and
//! its markers. clipboard-rs 0.3.5 reopens and clears the clipboard for
//! images and ignores errors from individual format writes.
use clipboard_rs::common::RustImage;
use clipboard_rs::{ClipboardContent, RustImageData};
use clipboard_win::formats::{Html, CF_DIB};
use clipboard_win::options;
use clipboard_win::raw;
use clipboard_win::Clipboard as WinClipboard;

use super::Result;

const CF_RTF: &str = "Rich Text Format";
const CF_PNG: &str = "PNG";
// A BMP file starts with a 14-byte file header; CF_DIB is the DIB that follows.
const BMP_FILE_HEADER_LEN: usize = 14;

pub(super) fn write(contents: &[ClipboardContent]) -> Result<()> {
    let _clip = WinClipboard::new_attempts(10)
        .map_err(|code| format!("Open clipboard error, code = {code}"))?;
    raw::empty().map_err(|code| format!("Empty clipboard error, code = {code}"))?;
    if let Err(error) = write_contents(contents) {
        // A partial write could lack the ownership token or confidentiality
        // marker, so leave the board empty instead of publishing it.
        let _ = raw::empty();
        return Err(error);
    }
    Ok(())
}

fn write_contents(contents: &[ClipboardContent]) -> Result<()> {
    for content in contents {
        match content {
            ClipboardContent::Text(text) => {
                raw::set_string_with(text, options::NoClear)
                    .map_err(|code| format!("Set text error, code = {code}"))?;
            }
            ClipboardContent::Html(html) => write_html(html)?,
            ClipboardContent::Rtf(rtf) => {
                let format = registered_format(CF_RTF)?;
                raw::set_without_clear(format, rtf.as_bytes())
                    .map_err(|code| format!("Set RTF error, code = {code}"))?;
            }
            ClipboardContent::Image(image) => write_image(image)?,
            ClipboardContent::Files(paths) => {
                raw::set_file_list_with(paths, options::NoClear)
                    .map_err(|code| format!("Set files error, code = {code}"))?;
            }
            ClipboardContent::Other(format, bytes) => {
                let code = registered_format(format)?;
                raw::set_without_clear(code, bytes)
                    .map_err(|code| format!("Set {format} error, code = {code}"))?;
            }
        }
    }
    Ok(())
}

fn write_image(image: &RustImageData) -> Result<()> {
    // Keep PNG for lossless consumers, then publish the DIB that Windows
    // applications expect. Setting CF_DIB directly avoids rebuilding the
    // bitmap through a GDI device context.
    let png = image
        .to_png()
        .map_err(|error| format!("Encode PNG error: {error}"))?;
    let png_format = registered_format(CF_PNG)?;
    raw::set_without_clear(png_format, png.get_bytes())
        .map_err(|code| format!("Set PNG image error, code = {code}"))?;

    let bitmap = image
        .to_bitmap()
        .map_err(|error| format!("Encode bitmap error: {error}"))?;
    let dib = bitmap
        .get_bytes()
        .get(BMP_FILE_HEADER_LEN..)
        .ok_or("Invalid bitmap data")?;
    raw::set_without_clear(CF_DIB, dib)
        .map_err(|code| format!("Set DIB image error, code = {code}"))?;
    Ok(())
}

fn registered_format(name: &str) -> Result<u32> {
    raw::register_format(name)
        .map(|format| format.get())
        .ok_or_else(|| format!("Could not register clipboard format {name}"))
}

fn write_html(html: &str) -> Result<()> {
    let format = Html::new()
        .ok_or("Could not register HTML clipboard format")?
        .code();
    let complete_document = html
        .trim_end()
        .as_bytes()
        .last_chunk::<7>()
        .is_some_and(|tag| tag.eq_ignore_ascii_case(b"</html>"));
    let result = if complete_document {
        // CF_HTML offsets are UTF-8 byte positions from the start of the header.
        // A captured document already has its own context. Keep it verbatim.
        let header = |start: usize, end: usize| {
            format!(
            "Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n"
        )
        };
        let start = header(0, 0).len();
        let mut data = header(start, start + html.len());
        data.push_str(html);
        data.push('\0');
        raw::set_without_clear(format, data.as_bytes())
    } else {
        raw::set_html_with(format, html, options::NoClear)
    };
    result.map_err(|code| format!("Set HTML error, code = {code}"))
}
