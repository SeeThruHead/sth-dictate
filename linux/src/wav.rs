pub const SAMPLE_RATE: u32 = 16_000;

const SHORTEST_SECONDS: f64 = 0.3;
const FULL_SCALE: f64 = 32_767.0;

pub fn samples_from_bytes(bytes: &[u8]) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|pair| i16::from_le_bytes(*pair)).collect()
}

pub fn too_short(samples: &[i16]) -> bool {
    (samples.len() as f64) < f64::from(SAMPLE_RATE) * SHORTEST_SECONDS
}

pub fn peak(samples: &[i16]) -> f64 {
    samples.iter().map(|sample| f64::from(*sample).abs()).fold(0.0, f64::max) / FULL_SCALE
}

pub fn louder(samples: &[i16]) -> Vec<i16> {
    let loudest = peak(samples);
    if loudest <= 0.001 {
        return samples.to_vec();
    }
    let gain = (0.9 / loudest).min(20.0);
    samples.iter().map(|sample| (f64::from(*sample) * gain).round().clamp(-FULL_SCALE, FULL_SCALE) as i16).collect()
}

pub fn silence(seconds: f64) -> Vec<i16> {
    vec![0; (seconds * f64::from(SAMPLE_RATE)) as usize]
}

pub fn wav(samples: &[i16]) -> Vec<u8> {
    let data_bytes = (samples.len() * 2) as u32;
    let header: [&[u8]; 13] = [
        b"RIFF",
        &(36 + data_bytes).to_le_bytes(),
        b"WAVE",
        b"fmt ",
        &16u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &SAMPLE_RATE.to_le_bytes(),
        &(SAMPLE_RATE * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
        &16u16.to_le_bytes(),
        b"data",
        &data_bytes.to_le_bytes(),
    ];
    header.concat().into_iter().chain(samples.iter().flat_map(|sample| sample.to_le_bytes())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_16_khz_mono_pcm() {
        let bytes = wav(&[1, -1, 0]);
        assert_eq!(bytes.len(), 50);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[24..28], &[0x80, 0x3e, 0, 0]);
        assert_eq!(&bytes[40..44], &6u32.to_le_bytes());
        assert_eq!(&bytes[44..], &[1, 0, 0xff, 0xff, 0, 0]);
    }

    #[test]
    fn raw_bytes_become_samples() {
        assert_eq!(samples_from_bytes(&[2, 0, 0xfe, 0xff, 7]), [2, -2]);
    }

    #[test]
    fn quiet_audio_is_raised_to_near_full_scale() {
        assert_eq!(louder(&[3277, -1638]), [29_490, -14_741]);
    }

    #[test]
    fn very_quiet_audio_is_raised_at_most_20_times() {
        assert_eq!(louder(&[100]), [2000]);
    }

    #[test]
    fn silence_is_left_alone() {
        assert_eq!(louder(&[16, 0]), [16, 0]);
    }

    #[test]
    fn a_recording_under_a_third_of_a_second_is_too_short() {
        assert!(too_short(&silence(0.25)));
        assert!(!too_short(&silence(0.32)));
    }
}
