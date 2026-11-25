use std::process::Command;

/// The command-line tool used to read and write the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardBackend {
    /// `wl-paste` / `wl-copy` from wl-clipboard.
    Wayland,
    /// `xclip` on the X11 CLIPBOARD selection.
    X11,
}

impl ClipboardBackend {
    /// Wayland when `WAYLAND_DISPLAY` is set, X11 when only `DISPLAY` is.
    pub fn detect() -> Self {
        Self::from_env(
            std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()),
            std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()),
        )
    }

    pub fn from_env(wayland: bool, x11: bool) -> Self {
        if x11 && !wayland {
            Self::X11
        } else {
            Self::Wayland
        }
    }

    /// The program this backend runs, for error messages.
    pub fn tool(self) -> &'static str {
        match self {
            Self::Wayland => "wl-clipboard (wl-paste/wl-copy)",
            Self::X11 => "xclip",
        }
    }

    /// Command that prints the clipboard's MIME types / X11 targets, one per line.
    pub fn list_types_args(self) -> Vec<&'static str> {
        match self {
            Self::Wayland => vec!["wl-paste", "--list-types"],
            Self::X11 => vec!["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"],
        }
    }

    /// Command that prints the clipboard content as `mime_type`, or as text.
    pub fn read_args(self, mime_type: Option<&str>) -> Vec<&str> {
        match (self, mime_type) {
            (Self::Wayland, Some(mime)) => vec!["wl-paste", "--type", mime],
            (Self::Wayland, None) => vec!["wl-paste", "--no-newline"],
            (Self::X11, Some(mime)) => vec!["xclip", "-selection", "clipboard", "-t", mime, "-o"],
            (Self::X11, None) => vec!["xclip", "-selection", "clipboard", "-o"],
        }
    }

    /// Command that takes the new clipboard content on stdin.
    pub fn write_args(self, mime_type: Option<&str>) -> Vec<&str> {
        match (self, mime_type) {
            (Self::Wayland, Some(mime)) => vec!["wl-copy", "--type", mime],
            (Self::Wayland, None) => vec!["wl-copy"],
            (Self::X11, Some(mime)) => vec!["xclip", "-selection", "clipboard", "-t", mime, "-i"],
            (Self::X11, None) => vec!["xclip", "-selection", "clipboard", "-i"],
        }
    }
}

pub fn run_command(args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new(args[0]).args(&args[1..]).output().ok()?;
    if output.status.success() {
        Some(output.stdout)
    } else {
        None
    }
}

pub fn has_mime_type(types: &str, mime: &str) -> bool {
    types.lines().any(|line| line == mime)
}

pub fn detect_clipboard_content_type(types: &str) -> ClipboardContentType {
    let is_image = has_mime_type(types, "image/png")
        || has_mime_type(types, "image/jpeg")
        || has_mime_type(types, "image/gif");
    let is_text = has_mime_type(types, "text/plain")
        || has_mime_type(types, "text/plain;charset=utf-8")
        || has_mime_type(types, "UTF8_STRING")
        || has_mime_type(types, "TEXT")
        || has_mime_type(types, "STRING");
    let is_file = has_mime_type(types, "text/uri-list");

    if is_file {
        ClipboardContentType::File
    } else if is_image {
        ClipboardContentType::Image
    } else if is_text {
        ClipboardContentType::Text
    } else {
        ClipboardContentType::Unsupported
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ClipboardContentType {
    Text,
    Image,
    File,
    Unsupported,
}

pub fn get_image_format_from_types(types: &str) -> Option<&'static str> {
    if has_mime_type(types, "image/png") {
        Some("image/png")
    } else if has_mime_type(types, "image/jpeg") {
        Some("image/jpeg")
    } else if has_mime_type(types, "image/gif") {
        Some("image/gif")
    } else {
        None
    }
}

pub fn copy_image_to_clipboard(mime_type: &str, data: &[u8]) -> Result<(), String> {
    if !mime_type.starts_with("image/") {
        return Err("Invalid image MIME type".to_string());
    }

    if data.is_empty() {
        return Err("Empty image data".to_string());
    }

    copy_to_clipboard(Some(mime_type), data)
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    copy_to_clipboard(None, text.as_bytes())
}

fn copy_to_clipboard(mime_type: Option<&str>, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let args = ClipboardBackend::detect().write_args(mime_type);
    let program = args[0];
    // xclip stays in the background to serve the selection; it must not hold
    // on to our stdout/stderr.
    let mut child = Command::new(program)
        .args(&args[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn {program}: {e}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(data)
            .map_err(|e| format!("Failed to write clipboard data: {e}"))?;
    }

    let exit_status = child
        .wait()
        .map_err(|e| format!("Failed to wait for {program}: {e}"))?;

    if exit_status.success() {
        Ok(())
    } else {
        Err(format!("{program} command failed"))
    }
}

/// Compute scaled dimensions for an image to fit within `max_w` x `max_h` while preserving
/// aspect ratio. Returns (width, height) in the same units as inputs. If inputs are invalid
/// (<= 0) returns (0, 0).
///
/// When `allow_upscale` is false the returned dimensions will not exceed the original image size.
pub fn compute_scaled_dimensions(
    orig_w: i32,
    orig_h: i32,
    max_w: i32,
    max_h: i32,
    allow_upscale: bool,
) -> (i32, i32) {
    if orig_w <= 0 || orig_h <= 0 || max_w <= 0 || max_h <= 0 {
        return (0, 0);
    }

    let scale_w = max_w as f64 / orig_w as f64;
    let scale_h = max_h as f64 / orig_h as f64;
    let mut scale = scale_w.min(scale_h);

    if !allow_upscale && scale > 1.0 {
        scale = 1.0;
    }

    let new_w = (orig_w as f64 * scale).round() as i32;
    let new_h = (orig_h as f64 * scale).round() as i32;

    // Ensure we never return negative or zero sizes for valid inputs
    (new_w.max(0), new_h.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_prefers_wayland_and_falls_back_to_x11() {
        assert_eq!(
            ClipboardBackend::from_env(true, true),
            ClipboardBackend::Wayland
        );
        assert_eq!(
            ClipboardBackend::from_env(true, false),
            ClipboardBackend::Wayland
        );
        assert_eq!(
            ClipboardBackend::from_env(false, true),
            ClipboardBackend::X11
        );
        // Nothing set: keep the old behaviour and let wl-paste report the error.
        assert_eq!(
            ClipboardBackend::from_env(false, false),
            ClipboardBackend::Wayland
        );
    }

    #[test]
    fn x11_backend_uses_the_clipboard_selection() {
        let x11 = ClipboardBackend::X11;
        assert_eq!(
            x11.list_types_args(),
            ["xclip", "-selection", "clipboard", "-t", "TARGETS", "-o"]
        );
        assert_eq!(
            x11.read_args(Some("image/png")),
            ["xclip", "-selection", "clipboard", "-t", "image/png", "-o"]
        );
        assert_eq!(
            x11.read_args(None),
            ["xclip", "-selection", "clipboard", "-o"]
        );
        assert_eq!(
            x11.write_args(Some("image/png")),
            ["xclip", "-selection", "clipboard", "-t", "image/png", "-i"]
        );
    }

    #[test]
    fn x11_targets_are_detected() {
        let targets = "TIMESTAMP\nTARGETS\nMULTIPLE\nUTF8_STRING\nSTRING\n";
        assert_eq!(
            detect_clipboard_content_type(targets),
            ClipboardContentType::Text
        );
        let image = "TIMESTAMP\nTARGETS\nimage/png\n";
        assert_eq!(
            detect_clipboard_content_type(image),
            ClipboardContentType::Image
        );
    }

    #[test]
    fn test_has_mime_type_single_line() {
        let types = "text/plain";
        assert!(has_mime_type(types, "text/plain"));
        assert!(!has_mime_type(types, "image/png"));
    }

    #[test]
    fn test_has_mime_type_multiple_lines() {
        let types = "text/plain\nimage/png\ntext/html";
        assert!(has_mime_type(types, "text/plain"));
        assert!(has_mime_type(types, "image/png"));
        assert!(has_mime_type(types, "text/html"));
        assert!(!has_mime_type(types, "image/jpeg"));
    }

    #[test]
    fn test_has_mime_type_empty() {
        let types = "";
        assert!(!has_mime_type(types, "text/plain"));
    }

    #[test]
    fn test_detect_clipboard_content_type_text() {
        let types = "text/plain\nUTF8_STRING";
        assert_eq!(
            detect_clipboard_content_type(types),
            ClipboardContentType::Text
        );
    }

    #[test]
    fn test_detect_clipboard_content_type_image() {
        let types = "image/png\nimage/jpeg";
        assert_eq!(
            detect_clipboard_content_type(types),
            ClipboardContentType::Image
        );
    }

    #[test]
    fn test_detect_clipboard_content_type_file() {
        let types = "text/uri-list\ntext/plain";
        assert_eq!(
            detect_clipboard_content_type(types),
            ClipboardContentType::File
        );
    }

    #[test]
    fn test_detect_clipboard_content_type_unsupported() {
        let types = "application/octet-stream\napplication/pdf";
        assert_eq!(
            detect_clipboard_content_type(types),
            ClipboardContentType::Unsupported
        );
    }

    #[test]
    fn test_get_image_format_from_types() {
        assert_eq!(get_image_format_from_types("image/png"), Some("image/png"));
        assert_eq!(
            get_image_format_from_types("image/jpeg"),
            Some("image/jpeg")
        );
        assert_eq!(get_image_format_from_types("image/gif"), Some("image/gif"));
        assert_eq!(get_image_format_from_types("text/plain"), None);
    }

    #[test]
    fn test_get_image_format_priority() {
        // PNG should have priority when multiple formats are available
        let types = "image/jpeg\nimage/png\nimage/gif";
        assert_eq!(get_image_format_from_types(types), Some("image/png"));
    }

    #[test]
    fn test_copy_image_to_clipboard_validation() {
        // Test invalid MIME type
        let result = copy_image_to_clipboard("text/plain", &[1, 2, 3]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid image MIME type");

        // Test empty data
        let result = copy_image_to_clipboard("image/png", &[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Empty image data");
    }

    // Tests for the new compute_scaled_dimensions helper

    #[test]
    fn test_compute_scaled_dimensions_downscale() {
        // original bigger than max, exact downscale
        let (w, h) = compute_scaled_dimensions(800, 600, 400, 300, false);
        assert_eq!(w, 400);
        assert_eq!(h, 300);
    }

    #[test]
    fn test_compute_scaled_dimensions_preserve_ratio() {
        // width-limited scaling: 1024x512 -> fit into 500x500 => 500x250
        let (w, h) = compute_scaled_dimensions(1024, 512, 500, 500, false);
        assert_eq!(w, 500);
        assert_eq!(h, 250);
    }

    #[test]
    fn test_compute_scaled_dimensions_no_upscale() {
        // When upscaling is disabled, result shouldn't exceed original
        let (w, h) = compute_scaled_dimensions(200, 100, 800, 800, false);
        assert_eq!(w, 200);
        assert_eq!(h, 100);
    }

    #[test]
    fn test_compute_scaled_dimensions_allow_upscale() {
        // When upscaling is allowed, image should scale up to max dimensions
        let (w, h) = compute_scaled_dimensions(200, 100, 800, 800, true);
        assert_eq!(w, 800);
        assert_eq!(h, 400);
    }

    #[test]
    fn test_compute_scaled_dimensions_invalid_inputs() {
        assert_eq!(compute_scaled_dimensions(0, 100, 100, 100, true), (0, 0));
        assert_eq!(compute_scaled_dimensions(100, -1, 100, 100, true), (0, 0));
        assert_eq!(compute_scaled_dimensions(100, 100, 0, 100, true), (0, 0));
    }
}
