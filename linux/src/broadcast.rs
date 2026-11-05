use serde::Serialize;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    pub state: Phase,
    pub message: String,
}

pub trait Publish: Send {
    fn publish(&self, status: &Status);
}

pub fn line(status: &Status) -> String {
    format!("{}\n", serde_json::to_string(status).unwrap_or_default())
}

pub fn socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    Path::new(&runtime).join("sth-dictate.sock")
}

#[derive(Default)]
struct Listeners {
    current: String,
    streams: Vec<UnixStream>,
}

pub struct Socket {
    listeners: Arc<Mutex<Listeners>>,
}

impl Socket {
    pub fn bind(path: &Path) -> Result<Socket, String> {
        let _ = std::fs::remove_file(path);
        let listener =
            UnixListener::bind(path).map_err(|error| format!("Could not listen on {}: {error}", path.display()))?;
        let listeners: Arc<Mutex<Listeners>> = Arc::default();
        let accepting = listeners.clone();
        std::thread::spawn(move || {
            listener.incoming().filter_map(Result::ok).for_each(|stream| welcome(&accepting, stream));
        });
        Ok(Socket { listeners })
    }
}

fn welcome(listeners: &Mutex<Listeners>, mut stream: UnixStream) {
    let Ok(mut listeners) = listeners.lock() else { return };
    if stream.write_all(listeners.current.as_bytes()).is_err() {
        return;
    }
    let streams = std::mem::take(&mut listeners.streams);
    listeners.streams = streams.into_iter().chain([stream]).collect();
}

impl Publish for Socket {
    fn publish(&self, status: &Status) {
        let Ok(mut listeners) = self.listeners.lock() else { return };
        let text = line(status);
        let streams = std::mem::take(&mut listeners.streams);
        *listeners = Listeners {
            current: text.clone(),
            streams: streams
                .into_iter()
                .filter_map(|mut stream| stream.write_all(text.as_bytes()).ok().map(|_| stream))
                .collect(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_one_json_line() {
        let status = Status { state: Phase::Recording, message: "Listening…".into() };
        assert_eq!(line(&status), "{\"state\":\"recording\",\"message\":\"Listening…\"}\n");
    }
}
