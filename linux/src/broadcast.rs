use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Streaming,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    pub state: Phase,
    pub message: String,
    pub pinned: bool,
    pub pinned_to: String,
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
    pub fn bind(path: &Path, commands: Sender<String>) -> Result<Socket, String> {
        let _ = std::fs::remove_file(path);
        let listener =
            UnixListener::bind(path).map_err(|error| format!("Could not listen on {}: {error}", path.display()))?;
        let listeners: Arc<Mutex<Listeners>> = Arc::default();
        let accepting = listeners.clone();
        std::thread::spawn(move || {
            listener.incoming().filter_map(Result::ok).for_each(|stream| {
                hear_commands(&stream, commands.clone());
                welcome(&accepting, stream);
            });
        });
        Ok(Socket { listeners })
    }
}

fn hear_commands(stream: &UnixStream, commands: Sender<String>) {
    let Ok(reader) = stream.try_clone() else { return };
    std::thread::spawn(move || {
        BufReader::new(reader)
            .lines()
            .map_while(Result::ok)
            .take_while(|line| commands.send(line.clone()).is_ok())
            .for_each(drop);
    });
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
        let status =
            Status { state: Phase::Streaming, message: "Listening…".into(), pinned: true, pinned_to: "Slack".into() };
        assert_eq!(
            line(&status),
            "{\"state\":\"streaming\",\"message\":\"Listening…\",\"pinned\":true,\"pinned_to\":\"Slack\"}\n"
        );
    }
}
