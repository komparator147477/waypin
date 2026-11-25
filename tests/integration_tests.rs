use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use tempfile::TempDir;

/// A directory with a mock `program` that runs `script`.
fn mock_program(program: &str, script: &str) -> TempDir {
    let temp_dir = tempfile::tempdir().expect("failed to create temporary directory");
    let path = temp_dir.path().join(program);
    fs::write(&path, script).expect("failed to create mock program");

    let mut permissions = fs::metadata(&path)
        .expect("failed to read mock program metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("failed to make mock program executable");

    temp_dir
}

fn mock_wl_paste(script: &str) -> TempDir {
    mock_program("wl-paste", script)
}

/// waypin in a Wayland session, with only the mock tools on PATH.
fn waypin_command(mock_bin_dir: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_waypin"));
    command
        .env("PATH", mock_bin_dir.path())
        .env("WAYLAND_DISPLAY", "wayland-mock");
    command
}

/// waypin in an X11 session (no WAYLAND_DISPLAY), with only the mock tools on PATH.
fn waypin_x11_command(mock_bin_dir: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_waypin"));
    command
        .env("PATH", mock_bin_dir.path())
        .env_remove("WAYLAND_DISPLAY")
        .env("DISPLAY", ":99");
    command
}

#[test]
fn arguments_print_usage_without_initializing_gtk() {
    let output = Command::new(env!("CARGO_BIN_EXE_waypin"))
        .arg("--help")
        .output()
        .expect("failed to execute waypin");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[cfg(feature = "global-shortcuts")]
#[test]
fn background_mode_fails_without_wayland_before_initializing_gtk() {
    let bin = env!("CARGO_BIN_EXE_waypin");
    let runtime_dir = tempfile::tempdir().expect("failed to create empty runtime directory");

    let output = Command::new(bin)
        .arg("--background")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env("XDG_RUNTIME_DIR", runtime_dir.path())
        .output()
        .expect("failed to execute waypin");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with(
            "waypin: background shortcut listener failed: could not connect to the Wayland display:"
        ),
        "unexpected stderr: {stderr}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn empty_clipboard_is_reported_without_initializing_gtk() {
    let mock_bin_dir = mock_wl_paste("#!/bin/sh\nexit 1\n");
    let output = waypin_command(&mock_bin_dir)
        .output()
        .expect("failed to execute waypin");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Could not retrieve clipboard types or clipboard is empty."));
}

#[test]
fn unsupported_clipboard_is_reported_without_initializing_gtk() {
    let mock_bin_dir = mock_wl_paste("#!/bin/sh\nprintf 'application/pdf\\n'\n");
    let output = waypin_command(&mock_bin_dir)
        .output()
        .expect("failed to execute waypin");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Clipboard does not contain supported image or text types."));
}

#[test]
fn x11_reads_targets_with_xclip() {
    // The mock only answers a TARGETS request on the CLIPBOARD selection.
    let mock_bin_dir = mock_program(
        "xclip",
        "#!/bin/sh\n[ \"$*\" = \"-selection clipboard -t TARGETS -o\" ] || exit 1\n\
         printf 'TARGETS\\napplication/pdf\\n'\n",
    );
    let output = waypin_x11_command(&mock_bin_dir)
        .output()
        .expect("failed to execute waypin");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Clipboard does not contain supported image or text types."));
}

#[test]
fn x11_without_xclip_says_so() {
    let empty_bin_dir = tempfile::tempdir().expect("failed to create temporary directory");
    let output = waypin_x11_command(&empty_bin_dir)
        .output()
        .expect("failed to execute waypin");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("xclip is not installed"),
        "unexpected stderr: {stderr}"
    );
}

/// End to end on a real X server (CI runs the tests under xvfb): put text on
/// the CLIPBOARD selection with xclip and check that waypin finds it and opens
/// its window instead of reporting an empty clipboard. Skipped inside a
/// Wayland session and when xclip is missing.
#[test]
fn x11_previews_text_set_by_xclip() {
    use std::io::Write;
    use std::process::Stdio;
    use std::time::Duration;

    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_none() {
        return;
    }
    let Ok(mut xclip) = Command::new("xclip")
        .args(["-selection", "clipboard", "-i"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    xclip
        .stdin
        .take()
        .expect("xclip stdin")
        .write_all(b"hello from x11")
        .expect("failed to write to xclip");
    assert!(xclip.wait().expect("xclip failed").success());

    let mut waypin = Command::new(env!("CARGO_BIN_EXE_waypin"))
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute waypin");
    std::thread::sleep(Duration::from_secs(2));
    // Still running means the preview window is open.
    let still_running = waypin.try_wait().expect("try_wait failed").is_none();
    let _ = waypin.kill();
    let output = waypin.wait_with_output().expect("failed to collect output");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(still_running, "waypin exited early; stderr: {stderr}");
    assert!(
        !stderr.contains("Could not retrieve clipboard types"),
        "unexpected stderr: {stderr}"
    );
}

#[cfg(feature = "pin-on-top")]
mod pin_on_top_tests {
    #[test]
    fn layer_shell_probe_does_not_panic() {
        if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
            return;
        }
        if gtk::init().is_err() {
            return;
        }
        let _supported = gtk_layer_shell::is_supported();
    }

    #[test]
    fn pin_on_top_binary_runs_without_panic() {
        let bin = env!("CARGO_BIN_EXE_waypin");
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("DISPLAY=:99 timeout 5 {bin}")])
            .output();

        let Ok(output) = output else {
            return;
        };
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked"),
            "pin-on-top binary panicked; stderr: {stderr}"
        );
    }
}
