pub const FRAME: usize = 480;

const FRAME_SECONDS: f64 = 0.03;
const START_FRAMES: usize = 3;
const PREROLL_FRAMES: usize = 10;
const SHORTEST_SPEECH_FRAMES: usize = 10;
const TAIL_FRAMES: usize = 5;
const LONGEST_FRAMES: usize = 1000;
const FLOOR_START: f64 = 100.0;
const FLOOR_RATE: f64 = 0.02;
const ABOVE_FLOOR: f64 = 3.0;

const FILLERS: [&str; 6] = ["thank you", "thanks", "thanks for watching", "thank you for watching", "you", "bye"];

pub fn is_filler(text: &str) -> bool {
    let words: String =
        text.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect::<String>().to_lowercase();
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    words.is_empty() || FILLERS.contains(&words.as_str())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub pause_frames: usize,
    pub voice_level: f64,
}

impl Tuning {
    pub fn new(pause_seconds: f64, voice_level: f64) -> Tuning {
        Tuning { pause_frames: (pause_seconds / FRAME_SECONDS).round().max(1.0) as usize, voice_level }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Speaking,
    Piece(Vec<i16>),
    Noise,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Quiet { preroll: Vec<Vec<i16>>, voiced_run: usize },
    Speech { frames: Vec<Vec<i16>>, silent_run: usize, voiced: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Listener {
    floor: f64,
    phase: Phase,
}

impl Default for Listener {
    fn default() -> Listener {
        Listener { floor: FLOOR_START, phase: quiet() }
    }
}

fn quiet() -> Phase {
    Phase::Quiet { preroll: vec![], voiced_run: 0 }
}

pub fn level(frame: &[i16]) -> f64 {
    if frame.is_empty() {
        return 0.0;
    }
    let energy: f64 = frame.iter().map(|sample| f64::from(*sample).powi(2)).sum();
    (energy / frame.len() as f64).sqrt()
}

fn last_frames(frames: Vec<Vec<i16>>, count: usize) -> Vec<Vec<i16>> {
    let skip = frames.len().saturating_sub(count);
    frames.into_iter().skip(skip).collect()
}

impl Listener {
    pub fn hear(self, frame: Vec<i16>, tuning: Tuning) -> (Listener, Option<Heard>) {
        let loudness = level(&frame);
        let voiced = loudness > tuning.voice_level.max(self.floor * ABOVE_FLOOR);
        let floor = if voiced { self.floor } else { self.floor + (loudness - self.floor) * FLOOR_RATE };
        match self.phase {
            Phase::Quiet { preroll, voiced_run } => quiet_frame(floor, preroll, voiced_run, frame, voiced),
            Phase::Speech { frames, silent_run, voiced: count } => {
                speech_frame(floor, frames, silent_run, count, frame, voiced, tuning)
            }
        }
    }
}

fn quiet_frame(
    floor: f64,
    preroll: Vec<Vec<i16>>,
    voiced_run: usize,
    frame: Vec<i16>,
    voiced: bool,
) -> (Listener, Option<Heard>) {
    let preroll = last_frames(preroll.into_iter().chain([frame]).collect(), PREROLL_FRAMES);
    let run = if voiced { voiced_run + 1 } else { 0 };
    if run < START_FRAMES {
        return (Listener { floor, phase: Phase::Quiet { preroll, voiced_run: run } }, None);
    }
    let phase = Phase::Speech { frames: preroll, silent_run: 0, voiced: run };
    (Listener { floor, phase }, Some(Heard::Speaking))
}

fn speech_frame(
    floor: f64,
    frames: Vec<Vec<i16>>,
    silent_run: usize,
    voiced_count: usize,
    frame: Vec<i16>,
    voiced: bool,
    tuning: Tuning,
) -> (Listener, Option<Heard>) {
    let frames: Vec<Vec<i16>> = frames.into_iter().chain([frame]).collect();
    let silent_run = if voiced { 0 } else { silent_run + 1 };
    let voiced_count = voiced_count + usize::from(voiced);
    if silent_run < tuning.pause_frames && frames.len() < LONGEST_FRAMES {
        return (Listener { floor, phase: Phase::Speech { frames, silent_run, voiced: voiced_count } }, None);
    }
    let done = Listener { floor, phase: quiet() };
    if voiced_count < SHORTEST_SPEECH_FRAMES {
        return (done, Some(Heard::Noise));
    }
    let keep = frames.len() - silent_run.saturating_sub(TAIL_FRAMES);
    (done, Some(Heard::Piece(frames.into_iter().take(keep).flatten().collect())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(level: i16) -> Vec<i16> {
        (0..FRAME).map(|index| if index % 2 == 0 { level } else { -level }).collect()
    }

    fn listen(frames: Vec<Vec<i16>>, tuning: Tuning) -> Vec<Heard> {
        frames
            .into_iter()
            .fold((Listener::default(), vec![]), |(listener, heard), frame| {
                let (next, event) = listener.hear(frame, tuning);
                (next, heard.into_iter().chain(event).collect())
            })
            .1
    }

    fn repeat(frame: Vec<i16>, count: usize) -> Vec<Vec<i16>> {
        vec![frame; count]
    }

    fn tuning() -> Tuning {
        Tuning::new(0.3, 150.0)
    }

    #[test]
    fn a_pause_after_speech_sends_the_speech_with_a_little_before_and_after() {
        let frames = [repeat(tone(10), 20), repeat(tone(2000), 20), repeat(tone(10), 10)].concat();
        let heard = listen(frames, tuning());
        assert_eq!(heard.len(), 2);
        assert_eq!(heard[0], Heard::Speaking);
        let Heard::Piece(samples) = &heard[1] else { panic!("expected a piece, got {heard:?}") };
        assert_eq!(samples.len(), (PREROLL_FRAMES - START_FRAMES + 20 + TAIL_FRAMES) * FRAME);
    }

    #[test]
    fn a_short_click_is_noise() {
        let frames = [repeat(tone(10), 10), repeat(tone(2000), 4), repeat(tone(10), 15)].concat();
        assert_eq!(listen(frames, tuning()), [Heard::Speaking, Heard::Noise]);
    }

    #[test]
    fn quiet_room_noise_never_starts_speech() {
        assert!(listen(repeat(tone(120), 200), tuning()).is_empty());
    }

    #[test]
    fn short_gaps_inside_a_sentence_do_not_cut_it() {
        let words = [repeat(tone(2000), 15), repeat(tone(10), 5), repeat(tone(2000), 15)].concat();
        let frames = [words, repeat(tone(10), 10)].concat();
        let pieces = listen(frames, tuning()).into_iter().filter(|heard| matches!(heard, Heard::Piece(_))).count();
        assert_eq!(pieces, 1);
    }

    #[test]
    fn whisper_filler_heard_in_silence_is_recognised() {
        assert!(is_filler(" Thank you. "));
        assert!(is_filler("you"));
        assert!(is_filler("..."));
        assert!(!is_filler("thank you, now restart the stack"));
    }

    #[test]
    fn the_pause_is_counted_in_frames() {
        assert_eq!(Tuning::new(1.0, 150.0).pause_frames, 33);
        assert_eq!(Tuning::new(0.0, 150.0).pause_frames, 1);
    }
}
