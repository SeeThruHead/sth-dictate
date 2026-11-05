use crate::wav;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

const SIGINT: i32 = 2;

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

pub trait Capture: Send {
    fn finish(self: Box<Self>) -> Vec<i16>;
}

pub trait Microphone: Send {
    fn start(&self) -> Result<Box<dyn Capture>, String>;
}

pub struct PipeWire {
    pub target: Option<String>,
}

struct Recording {
    child: Child,
    collected: JoinHandle<Vec<u8>>,
}

impl Microphone for PipeWire {
    fn start(&self) -> Result<Box<dyn Capture>, String> {
        let rate = wav::SAMPLE_RATE.to_string();
        let target = self.target.iter().flat_map(|target| ["--target", target.as_str()]);
        let args =
            ["--raw", "--rate", &rate, "--channels", "1", "--format", "s16"].into_iter().chain(target).chain(["-"]);
        let mut child = Command::new("pw-record")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start pw-record: {error}"))?;
        let mut stdout = child.stdout.take().ok_or("pw-record gave no output")?;
        let collected = std::thread::spawn(move || {
            let mut bytes = vec![];
            stdout.read_to_end(&mut bytes).map(|_| bytes).unwrap_or_default()
        });
        Ok(Box::new(Recording { child, collected }))
    }
}

impl Capture for Recording {
    fn finish(self: Box<Self>) -> Vec<i16> {
        let Recording { mut child, collected } = *self;
        unsafe { kill(child.id() as i32, SIGINT) };
        let _ = child.wait();
        wav::samples_from_bytes(&collected.join().unwrap_or_default())
    }
}
