use std::io;

use interprocess::local_socket::tokio::{Listener, Stream, prelude::*};
use interprocess::local_socket::{ListenerOptions, Name};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SHOW: &[u8] = b"show\n";
const MAX_MESSAGE: u64 = 64;

pub enum Claim {
    First(Listener),
    Running,
}

#[cfg(not(windows))]
fn name() -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    let dir = thelemail_core::paths::data_root();
    std::fs::create_dir_all(&dir)?;
    dir.join("instance.sock").to_fs_name::<GenericFilePath>()
}

#[cfg(windows)]
fn name() -> io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    let user = std::env::var("USERNAME").unwrap_or_default();
    format!("com.thelemail.desktop.{user}").to_ns_name::<GenericNamespaced>()
}

pub async fn claim() -> io::Result<Claim> {
    if let Ok(mut stream) = Stream::connect(name()?).await {
        allow_focus_handoff();
        stream.write_all(SHOW).await?;
        stream.flush().await?;
        return Ok(Claim::Running);
    }
    let listener = ListenerOptions::new()
        .name(name()?)
        .try_overwrite(true)
        .create_tokio()?;
    Ok(Claim::First(listener))
}

pub async fn serve(listener: Listener, show: async_channel::Sender<()>) {
    loop {
        let stream = match listener.accept().await {
            Ok(stream) => stream,
            Err(err) => {
                tracing::warn!("instance: stopped listening: {err}");
                return;
            }
        };
        let show = show.clone();
        tokio::spawn(async move {
            let mut message = Vec::new();
            let read = stream.take(MAX_MESSAGE).read_to_end(&mut message).await;
            if read.is_ok() && message == SHOW {
                let _ = show.send(()).await;
            }
        });
    }
}

#[cfg(windows)]
fn allow_focus_handoff() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
}

#[cfg(not(windows))]
fn allow_focus_handoff() {}
