//! The bridge between the browser extension and BDownloader.
//!
//! The browser starts this program for each message the extension sends
//! (Chrome's "native messaging"). It reads the message from stdin, passes it
//! to the app over the named pipe, starting the app if needed, and writes the
//! reply to stdout. Only the extension listed in the host manifest can start it.

// No console window; the browser talks to it through redirected stdin and stdout.
#![windows_subsystem = "windows"]

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use bdownloader_ipc::{MAX_MESSAGE, Reply, Request};
use serde_json::json;

/// How long to wait for the app to start listening after launching it.
const STARTUP_WAIT: Duration = Duration::from_secs(20);

fn main() {
    let reply = match handle(&mut io::stdin().lock()) {
        Ok(Reply { ok: true, .. }) => json!({ "ok": true }),
        Ok(Reply { error, .. }) => json!({ "ok": false, "error": error }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };
    let _ = write_message(&mut io::stdout().lock(), &serde_json::to_vec(&reply).unwrap_or_default());
}

fn handle(input: &mut impl Read) -> io::Result<Reply> {
    let message = read_message(input)?;
    let request: Request = serde_json::from_slice(&message).map_err(io::Error::other)?;
    let taken = Reply { ok: true, error: None };
    match &request {
        Request::Add(add) if !add.is_valid() => return Err(io::Error::other("only http and https links and magnet links can be downloaded")),
        // Files on the PC are opened from Explorer, never by a web page.
        Request::OpenTorrentFile { .. } => return Err(io::Error::other("the browser can't open files on the PC")),
        // Don't start the app just because the browser started.
        Request::Hello { .. } | Request::Show => return bdownloader_ipc::send(&request).map(|()| taken.clone()).or(Ok(taken)),
        Request::Add(_) => {}
    }
    // A download the browser is holding waits for the app to check its link.
    let expects = matches!(&request, Request::Add(add) if add.expect.is_some());
    let deliver = || {
        if expects {
            bdownloader_ipc::ask(&request)
        } else {
            bdownloader_ipc::send(&request).map(|()| taken.clone())
        }
    };
    let reply = match deliver() {
        Ok(reply) => reply,
        Err(_) => {
            launch_app()?;
            let deadline = Instant::now() + STARTUP_WAIT;
            loop {
                sleep(Duration::from_millis(200));
                match deliver() {
                    Ok(reply) => break reply,
                    Err(e) if Instant::now() >= deadline => return Err(e),
                    Err(_) => {}
                }
            }
        }
    };
    // Lets the app show the extension as connected, even if it wasn't
    // running when the browser started and said hello.
    let _ = bdownloader_ipc::send(&Request::Hello { browser: None });
    Ok(reply)
}

/// Reads one message: a 32-bit length in native byte order, then that many bytes of JSON.
fn read_message(input: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::other("message too large"));
    }
    let mut message = vec![0; len];
    input.read_exact(&mut message)?;
    Ok(message)
}

fn write_message(output: &mut impl Write, message: &[u8]) -> io::Result<()> {
    output.write_all(&(message.len() as u32).to_ne_bytes())?;
    output.write_all(message)?;
    output.flush()
}

/// Starts BDownloader.exe from the same folder as this program.
fn launch_app() -> io::Result<()> {
    let app = app_path()?;
    let command = || {
        let mut command = Command::new(&app);
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        command
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        keep_pipes_private();
        // The browser runs this program in a job that is closed when it
        // exits. Break away from it, or the app would close with us.
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        if command().creation_flags(CREATE_BREAKAWAY_FROM_JOB).spawn().is_ok() {
            return Ok(());
        }
    }
    command().spawn().map(drop)
}

/// Stops the browser's pipes from being inherited by the app. Otherwise the
/// app would hold them open for as long as it runs, and the browser would
/// keep waiting on this program.
#[cfg(windows)]
fn keep_pipes_private() {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    const HANDLE_FLAG_INHERIT: u32 = 1;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    }
    for handle in [io::stdin().as_raw_handle(), io::stdout().as_raw_handle(), io::stderr().as_raw_handle()] {
        // SAFETY: the handles belong to this process and stay open; only the inherit flag changes.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
    }
}

fn app_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.with_file_name(if cfg!(windows) { "BDownloader.exe" } else { "BDownloader" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(json: &str) -> Vec<u8> {
        let mut bytes = (json.len() as u32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(json.as_bytes());
        bytes
    }

    #[test]
    fn reads_and_writes_frames() {
        let mut out = Vec::new();
        write_message(&mut out, br#"{"ok":true}"#).unwrap();
        assert_eq!(read_message(&mut out.as_slice()).unwrap(), br#"{"ok":true}"#);
    }

    #[test]
    fn rejects_oversized_and_truncated_messages() {
        let huge = ((MAX_MESSAGE + 1) as u32).to_ne_bytes();
        assert!(read_message(&mut huge.as_slice()).is_err());
        let truncated = &framed(r#"{"type":"show"}"#)[..10];
        assert!(read_message(&mut &truncated[..]).is_err());
    }

    #[test]
    fn refuses_file_urls_before_contacting_the_app() {
        let message = framed(r#"{"type":"add","url":"file:///C:/secret.txt"}"#);
        let err = handle(&mut message.as_slice()).unwrap_err();
        assert!(err.to_string().contains("http"));
    }

    #[test]
    fn refuses_unknown_messages() {
        assert!(handle(&mut framed(r#"{"type":"delete_everything"}"#).as_slice()).is_err());
    }
}
