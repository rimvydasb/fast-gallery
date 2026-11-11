//! `thumb://` and `image://` protocol handlers (PERF-13). Image bytes never pass through IPC.
//!
//! URL form: `thumb://localhost/<percent-encoded absolute path>?s=256&v=<mtime>` and
//! `image://localhost/<percent-encoded absolute path>?v=<mtime>`.
//! The handlers serve only files with a supported image extension (SEC-03).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use percent_encoding::percent_decode_str;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

use crate::decode;
use crate::scan;
use crate::thumbs::{Lookup, ThumbnailService};
use crate::AppState;

fn parse(req: &Request<Vec<u8>>) -> Option<(PathBuf, Option<u32>)> {
    let uri = req.uri();
    let raw = uri.path().trim_start_matches('/');
    let decoded = percent_decode_str(raw).decode_utf8().ok()?;
    let path = PathBuf::from(decoded.as_ref());
    if !path.is_absolute() {
        return None;
    }
    let size = uri
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("s=")))
        .and_then(|v| v.parse().ok());
    Some((path, size))
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*").body(Vec::new()).unwrap()
}

fn ok(bytes: Vec<u8>, mime: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        // The URL carries the file mtime, so the response never changes.
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(bytes)
        .unwrap()
}

fn io_status(e: &std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        std::io::ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub fn thumb<R: Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let Some((path, size)) = parse(&req) else {
        return responder.respond(status(StatusCode::BAD_REQUEST));
    };
    if !scan::is_image_path(&path) {
        return responder.respond(status(StatusCode::FORBIDDEN));
    }
    let svc: Arc<ThumbnailService> = Arc::clone(&ctx.app_handle().state::<AppState>().thumbs);
    let size = size.unwrap_or(256);
    crate::pools::io().spawn(move || match svc.lookup(&path, size) {
        Err(e) => responder.respond(status(io_status(&e))),
        Ok(Lookup::Hit(e)) => responder.respond(ok(e.bytes.clone(), e.mime)),
        Ok(Lookup::Miss(key)) => crate::pools::decode().spawn(move || match svc.generate(key) {
            Some(e) => responder.respond(ok(e.bytes.clone(), e.mime)),
            None => responder.respond(status(StatusCode::UNPROCESSABLE_ENTITY)),
        }),
    });
}

/// MIME type of formats the WebView (WKWebView) displays natively on this macOS version.
pub fn native_mime(path: &Path) -> Option<&'static str> {
    let ext = scan::ext_lower(path)?;
    let os = crate::pools::macos_major();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "heic" | "heif" if os >= 14 => "image/heic",
        "avif" if os >= 13 => "image/avif",
        _ => return None,
    })
}

/// `w` query parameter: the viewer asks for a display-size version with at least this long edge.
fn display_size(req: &Request<Vec<u8>>) -> Option<u32> {
    req.uri().query()?.split('&').find_map(|kv| kv.strip_prefix("w="))?.parse().ok()
}

pub fn image<R: Runtime>(_ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let Some((path, _)) = parse(&req) else {
        return responder.respond(status(StatusCode::BAD_REQUEST));
    };
    if !scan::is_image_path(&path) {
        return responder.respond(status(StatusCode::FORBIDDEN));
    }
    if let Some(w) = display_size(&req) {
        // PERF-10: a subsampled decode is much faster than a full decode in the WebView.
        return crate::pools::decode().spawn(move || match decode::screen_image(&path, w) {
            Some(e) => responder.respond(ok(e.bytes, e.mime)),
            None => match native_mime(&path).map(|m| (std::fs::read(&path), m)) {
                Some((Ok(bytes), mime)) => responder.respond(ok(bytes, mime)),
                Some((Err(e), _)) => responder.respond(status(io_status(&e))),
                None => match decode::transcode_full(&path) {
                    Some(e) => responder.respond(ok(e.bytes, e.mime)),
                    None => responder.respond(status(StatusCode::UNPROCESSABLE_ENTITY)),
                },
            },
        });
    }
    match native_mime(&path) {
        Some(mime) => crate::pools::io().spawn(move || match std::fs::read(&path) {
            Ok(bytes) => responder.respond(ok(bytes, mime)),
            Err(e) => responder.respond(status(io_status(&e))),
        }),
        // TIFF, RAW, and HEIC/AVIF on older macOS: decode with ImageIO.
        None => crate::pools::decode().spawn(move || match decode::transcode_full(&path) {
            Some(e) => responder.respond(ok(e.bytes, e.mime)),
            None if !path.exists() => responder.respond(status(StatusCode::NOT_FOUND)),
            None => responder.respond(status(StatusCode::UNPROCESSABLE_ENTITY)),
        }),
    }
}

