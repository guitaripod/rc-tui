use std::f64::consts::TAU;

/// Sample rate of the chime in hertz.
pub const SAMPLE_RATE: u32 = 48_000;

/// Total length of the chime in seconds.
const DURATION_SECONDS: f64 = 1.8;

/// Linear fade-out applied to the tail so the last sample is exactly zero, in seconds.
const FADE_OUT_SECONDS: f64 = 0.12;

/// Linear attack applied to every note to avoid clicks, in seconds.
const ATTACK_SECONDS: f64 = 0.004;

/// Absolute peak the chime is normalized to before conversion (about -6 dBFS).
const PEAK: f64 = 0.5;

/// Length of the canonical PCM WAV header in bytes.
const WAV_HEADER_LEN: u32 = 44;

/// Decay time constant of the fundamental for the first three notes, in seconds.
const BELL_DECAY_SECONDS: f64 = 0.42;

/// Decay time constant of the fundamental for the final note, in seconds.
const FINAL_DECAY_SECONDS: f64 = 0.9;

/// Relative loudness of the first three notes.
const BELL_GAIN: f64 = 0.8;

/// Relative loudness of the final note: 0.9 times the 1.1 lift that makes it slightly louder.
const FINAL_GAIN: f64 = 0.9 * 1.1;

/// Exponent applied to each partial's ratio when shortening its decay time constant.
const PARTIAL_DECAY_EXPONENT: f64 = 0.8;

/// Vibrato rate applied to the final note's fundamental, in hertz.
const VIBRATO_HZ: f64 = 5.5;

/// Vibrato depth as a fraction of the fundamental frequency (0.15 percent).
const VIBRATO_DEPTH: f64 = 0.0015;

/// Bell partials as (frequency ratio, amplitude) pairs; the third is slightly inharmonic.
const PARTIALS: [(f64, f64); 4] = [(1.0, 1.0), (2.0, 0.42), (3.01, 0.18), (4.16, 0.07)];

/// One struck bell note of the rising arpeggio.
struct Note {
    /// Fundamental frequency in hertz.
    frequency: f64,
    /// Onset relative to the start of the chime, in seconds.
    onset_seconds: f64,
    /// Decay time constant of the fundamental, in seconds.
    decay_seconds: f64,
    /// Relative loudness of the whole note.
    gain: f64,
    /// Whether the fundamental carries the shimmer vibrato.
    vibrato: bool,
}

/// The G5, B5, D6 arpeggio followed by a longer, louder G6.
const NOTES: [Note; 4] = [
    Note {
        frequency: 783.99,
        onset_seconds: 0.0,
        decay_seconds: BELL_DECAY_SECONDS,
        gain: BELL_GAIN,
        vibrato: false,
    },
    Note {
        frequency: 987.77,
        onset_seconds: 0.085,
        decay_seconds: BELL_DECAY_SECONDS,
        gain: BELL_GAIN,
        vibrato: false,
    },
    Note {
        frequency: 1174.66,
        onset_seconds: 0.170,
        decay_seconds: BELL_DECAY_SECONDS,
        gain: BELL_GAIN,
        vibrato: false,
    },
    Note {
        frequency: 1567.98,
        onset_seconds: 0.255,
        decay_seconds: FINAL_DECAY_SECONDS,
        gain: FINAL_GAIN,
        vibrato: true,
    },
];

/// Mono samples of the chime in the range [-1, 1], peak-normalized to 0.5.
pub fn samples() -> Vec<f32> {
    let mut buffer = vec![0.0_f64; seconds_to_samples(DURATION_SECONDS)];
    for note in &NOTES {
        render_note(&mut buffer, note);
    }
    apply_fade_out(&mut buffer);
    normalize(&mut buffer);
    buffer.into_iter().map(|sample| sample as f32).collect()
}

/// The chime as a canonical 44-byte-header 16-bit mono 48 kHz PCM WAV file.
pub fn wav() -> Vec<u8> {
    let data: Vec<u8> = samples()
        .into_iter()
        .flat_map(|sample| to_pcm(sample).to_le_bytes())
        .collect();
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let block_align = channels * bits_per_sample / 8;
    let byte_rate = SAMPLE_RATE * u32::from(block_align);
    let data_len = u32::try_from(data.len()).expect("chime data fits in a WAV data chunk");
    let riff_len = WAV_HEADER_LEN - 8 + data_len;

    let mut bytes = Vec::with_capacity(WAV_HEADER_LEN as usize + data.len());
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&riff_len.to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

/// Adds one bell note into the buffer, starting at its onset and ringing to the end.
fn render_note(buffer: &mut [f64], note: &Note) {
    let onset = seconds_to_samples(note.onset_seconds);
    for (index, sample) in buffer.iter_mut().enumerate().skip(onset) {
        let t = (index - onset) as f64 / f64::from(SAMPLE_RATE);
        *sample += note.gain * attack(t) * bell_value(note, t);
    }
}

/// Instantaneous bell amplitude of a note at `t` seconds after its onset.
fn bell_value(note: &Note, t: f64) -> f64 {
    PARTIALS
        .iter()
        .enumerate()
        .map(|(index, &(ratio, amplitude))| {
            let decay = note.decay_seconds / ratio.powf(PARTIAL_DECAY_EXPONENT);
            let phase = if index == 0 {
                fundamental_phase(note, t)
            } else {
                TAU * note.frequency * ratio * t
            };
            amplitude * (-t / decay).exp() * phase.sin()
        })
        .sum::<f64>()
}

/// Phase of the fundamental at `t` seconds, integrating the vibrato when the note carries it.
fn fundamental_phase(note: &Note, t: f64) -> f64 {
    let base = TAU * note.frequency;
    if note.vibrato {
        let angular = TAU * VIBRATO_HZ;
        base * (t + VIBRATO_DEPTH / angular * (1.0 - (angular * t).cos()))
    } else {
        base * t
    }
}

/// Linear ramp from silence to full level over `ATTACK_SECONDS`.
fn attack(t: f64) -> f64 {
    (t / ATTACK_SECONDS).min(1.0)
}

/// Ramps the final `FADE_OUT_SECONDS` linearly to zero, ending exactly on zero.
fn apply_fade_out(buffer: &mut [f64]) {
    let fade = seconds_to_samples(FADE_OUT_SECONDS);
    let len = buffer.len();
    for (index, sample) in buffer.iter_mut().enumerate().skip(len - fade) {
        *sample *= (len - 1 - index) as f64 / fade as f64;
    }
}

/// Scales the buffer so its absolute peak equals `PEAK`.
fn normalize(buffer: &mut [f64]) {
    let peak = buffer
        .iter()
        .fold(0.0_f64, |acc, sample| acc.max(sample.abs()));
    if peak > 0.0 {
        let scale = PEAK / peak;
        for sample in buffer.iter_mut() {
            *sample *= scale;
        }
    }
}

/// Converts a float sample to 16-bit PCM with rounding and clamping.
fn to_pcm(sample: f32) -> i16 {
    (f64::from(sample) * f64::from(i16::MAX))
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Number of samples at `SAMPLE_RATE` covering `seconds`, rounded to the nearest sample.
fn seconds_to_samples(seconds: f64) -> usize {
    (seconds * f64::from(SAMPLE_RATE)).round() as usize
}

/// Unit tests for the chime synthesis and WAV container.
#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a little-endian `u32` at `offset`.
    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
    }

    /// Reads a little-endian `u16` at `offset`.
    fn read_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("two bytes"))
    }

    /// Decodes the PCM payload of a WAV produced by `wav()`.
    fn pcm_from_wav(bytes: &[u8]) -> Vec<i16> {
        bytes[44..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    /// Mean square of a float sample slice.
    fn energy(samples: &[f32]) -> f64 {
        samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum()
    }

    /// The WAV header uses the canonical offsets and matches the byte length.
    #[test]
    fn wav_header_is_canonical_and_consistent() {
        let bytes = wav();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(read_u32(&bytes, 16), 16);
        assert_eq!(read_u16(&bytes, 20), 1);
        assert_eq!(read_u16(&bytes, 22), 1);
        assert_eq!(read_u32(&bytes, 24), 48_000);
        assert_eq!(read_u32(&bytes, 28), 96_000);
        assert_eq!(read_u16(&bytes, 32), 2);
        assert_eq!(read_u16(&bytes, 34), 16);
        assert_eq!(&bytes[36..40], b"data");
        let data_len = read_u32(&bytes, 40) as usize;
        assert_eq!(data_len, bytes.len() - 44);
        assert_eq!(read_u32(&bytes, 4) as usize, bytes.len() - 8);
        assert_eq!(data_len, samples().len() * 2);
    }

    /// The data chunk describes 1.8 seconds of audio within 1 ms.
    #[test]
    fn duration_from_data_size_is_1_8_seconds() {
        let bytes = wav();
        let data_len = read_u32(&bytes, 40);
        let seconds = f64::from(data_len / 2) / f64::from(SAMPLE_RATE);
        assert!((seconds - 1.8).abs() <= 0.001, "duration was {seconds}");
    }

    /// The peak lands on 0.5 full scale, about 16384 as i16.
    #[test]
    fn peak_is_about_minus_six_dbfs() {
        let peak = pcm_from_wav(&wav())
            .iter()
            .map(|s| i32::from(*s).abs())
            .max()
            .expect("non-empty");
        assert!((16_000..=16_500).contains(&peak), "peak was {peak}");
    }

    /// The chime starts and ends on silence.
    #[test]
    fn first_and_last_samples_are_near_zero() {
        let pcm = pcm_from_wav(&wav());
        assert!(pcm.first().expect("non-empty").abs() <= 50);
        assert!(pcm.last().expect("non-empty").abs() <= 50);
    }

    /// Every synthesized sample is finite and inside [-1, 1].
    #[test]
    fn samples_are_finite_and_in_range() {
        for sample in samples() {
            assert!(!sample.is_nan());
            assert!((-1.0..=1.0).contains(&sample));
        }
    }

    /// The last 0.3 seconds carry less energy than the first 0.5 seconds.
    #[test]
    fn tail_is_quieter_than_the_attack() {
        let samples = samples();
        let head = &samples[..24_000];
        let tail = &samples[samples.len() - 14_400..];
        assert!(energy(tail) < energy(head));
    }

    /// Writes the chime to `RC_TUI_CHIME_OUT` for listening; skipped when the variable is unset.
    #[test]
    #[ignore]
    fn writes_chime_to_disk() {
        if let Ok(path) = std::env::var("RC_TUI_CHIME_OUT") {
            std::fs::write(path, wav()).expect("write chime WAV");
        }
    }
}
