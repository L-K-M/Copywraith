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
            ClipboardContent::Html(html) => {
                let format = html_format()?;
                raw::set_html_with(format, html, options::NoClear)
                    .map_err(|code| format!("Set HTML error, code = {code}"))?;
            }
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

fn html_format() -> Result<u32> {
    Html::new()
        .map(|format| format.code())
        .ok_or_else(|| "Could not register HTML clipboard format".to_string())
}
