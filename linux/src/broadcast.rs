use crate::pin::Target;
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
    #[serde(skip)]
    pub target: Option<Target>,
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
    arrivals: u64,
    streams: Vec<(u64, UnixStream)>,
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
                let Ok(reader) = stream.try_clone() else { return };
                let Some(arrival) = welcome(&accepting, stream) else { return };
                hear_commands(reader, commands.clone(), accepting.clone(), arrival);
            });
        });
        Ok(Socket { listeners })
    }
}

fn hear_commands(reader: UnixStream, commands: Sender<String>, listeners: Arc<Mutex<Listeners>>, arrival: u64) {
    std::thread::spawn(move || {
        BufReader::new(reader)
            .lines()
            .map_while(Result::ok)
            .take_while(|line| commands.send(line.clone()).is_ok())
            .for_each(drop);
        forget(&listeners, arrival);
    });
}

fn welcome(listeners: &Mutex<Listeners>, mut stream: UnixStream) -> Option<u64> {
    let mut listeners = listeners.lock().ok()?;
    stream.write_all(listeners.current.as_bytes()).ok()?;
    let arrival = listeners.arrivals;
    let streams = std::mem::take(&mut listeners.streams);
    *listeners = Listeners {
        current: std::mem::take(&mut listeners.current),
        arrivals: arrival + 1,
        streams: streams.into_iter().chain([(arrival, stream)]).collect(),
    };
    Some(arrival)
}

fn forget(listeners: &Mutex<Listeners>, arrival: u64) {
    let Ok(mut listeners) = listeners.lock() else { return };
    let streams = std::mem::take(&mut listeners.streams);
    listeners.streams = streams.into_iter().filter(|(known, _)| *known != arrival).collect();
}

pub struct Publishers(pub Vec<Box<dyn Publish>>);

impl Publish for Publishers {
    fn publish(&self, status: &Status) {
        self.0.iter().for_each(|publisher| publisher.publish(status));
    }
}

impl Publish for Socket {
    fn publish(&self, status: &Status) {
        let Ok(mut listeners) = self.listeners.lock() else { return };
        let text = line(status);
        let streams = std::mem::take(&mut listeners.streams);
        *listeners = Listeners {
            current: text.clone(),
            arrivals: listeners.arrivals,
            streams: streams
                .into_iter()
                .filter_map(|(arrival, mut stream)| stream.write_all(text.as_bytes()).ok().map(|_| (arrival, stream)))
                .collect(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    fn open_files() -> usize {
        std::fs::read_dir("/proc/self/fd").map(Iterator::count).unwrap_or(0)
    }

    fn settles_at(wanted: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        std::iter::repeat_with(|| {
            std::thread::sleep(Duration::from_millis(20));
            open_files()
        })
        .take_while(|_| Instant::now() < deadline)
        .any(|open| open <= wanted)
    }

    #[test]
    fn a_client_that_leaves_costs_nothing_once_it_is_gone() {
        let folder = std::env::temp_dir().join(format!("sth-dictate-clients-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("socket");
        let (commands, inbox) = channel();
        let _socket = Socket::bind(&path, commands).unwrap();
        let before = open_files();

        (0..20).for_each(|_| {
            let mut client = UnixStream::connect(&path).unwrap();
            client.write_all(b"status\n").unwrap();
            assert_eq!(inbox.recv_timeout(Duration::from_secs(3)).as_deref(), Ok("status"));
        });

        assert!(settles_at(before), "{} open files before, {} after 20 clients came and went", before, open_files());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn status_is_one_json_line() {
        let status = Status {
            state: Phase::Streaming,
            message: "Listening…".into(),
            pinned: true,
            pinned_to: "Slack".into(),
            target: Some(Target::Herdr),
        };
        assert_eq!(
            line(&status),
            "{\"state\":\"streaming\",\"message\":\"Listening…\",\"pinned\":true,\"pinned_to\":\"Slack\"}\n"
        );
    }
}
