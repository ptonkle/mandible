//! Sounds, made by the core: a small integer synthesiser that writes each
//! cue as a complete WAV file (16-bit mono PCM). The page hands the bytes to
//! Web Audio and plays them; it holds no frequencies, durations or volumes.
//! Which cue plays, and when, is decided in `app.rs`.

use serde::{Deserialize, Serialize};

use crate::dice::Rng;
use crate::fixed::{sin64, STEPS, TRIG_ONE};

pub const SAMPLE_RATE: u32 = 22_050;

/// Every sound the game makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cue {
    /// Something cannot be done: not enough resources, Housing Full, Pop Cap,
    /// a blocked placement. Two short low beeps.
    Warning,
    /// An upgrade or Evolve finished. A rising chime.
    Complete,
    /// Our queen died, or our colony is attacked. A falling alarm.
    Alert,
    /// A Windfall or a Hill appeared. A bright arpeggio.
    Event,
    Victory,
    Defeat,
    /// A button or menu click.
    Click,
}

impl Cue {
    pub const ALL: [Cue; 7] = [
        Cue::Warning,
        Cue::Complete,
        Cue::Alert,
        Cue::Event,
        Cue::Victory,
        Cue::Defeat,
        Cue::Click,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Cue::Warning => "warning",
            Cue::Complete => "complete",
            Cue::Alert => "alert",
            Cue::Event => "event",
            Cue::Victory => "victory",
            Cue::Defeat => "defeat",
            Cue::Click => "click",
        }
    }

    pub fn from_key(k: &str) -> Option<Cue> {
        Cue::ALL.into_iter().find(|c| c.key() == k)
    }

    /// The fewest milliseconds between two plays of this cue.
    pub fn cooldown_ms(self) -> i64 {
        match self {
            Cue::Warning => 1_500,
            Cue::Alert => 6_000,
            Cue::Click => 0,
            _ => 500,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Wave {
    Sine,
    Square,
    Triangle,
    Noise,
}

/// One note: frequency in Hz, start and length in milliseconds, loudness in
/// percent of full scale.
#[derive(Debug, Clone, Copy)]
struct Note {
    hz: u32,
    at_ms: u32,
    ms: u32,
    loud: i32,
    wave: Wave,
}

const fn n(hz: u32, at_ms: u32, ms: u32, loud: i32, wave: Wave) -> Note {
    Note {
        hz,
        at_ms,
        ms,
        loud,
        wave,
    }
}

fn notes(cue: Cue) -> Vec<Note> {
    use Wave::*;
    match cue {
        Cue::Warning => vec![n(330, 0, 110, 55, Square), n(262, 150, 160, 55, Square)],
        Cue::Complete => vec![
            n(523, 0, 120, 45, Triangle),
            n(659, 90, 120, 45, Triangle),
            n(784, 180, 260, 45, Triangle),
        ],
        Cue::Alert => vec![
            n(880, 0, 160, 50, Square),
            n(660, 170, 160, 50, Square),
            n(880, 340, 160, 50, Square),
            n(660, 510, 220, 50, Square),
        ],
        Cue::Event => vec![
            n(587, 0, 100, 40, Sine),
            n(740, 80, 100, 40, Sine),
            n(880, 160, 100, 40, Sine),
            n(1175, 240, 240, 40, Sine),
        ],
        Cue::Victory => vec![
            n(523, 0, 160, 50, Triangle),
            n(659, 160, 160, 50, Triangle),
            n(784, 320, 160, 50, Triangle),
            n(1047, 480, 520, 55, Triangle),
            n(523, 480, 520, 30, Sine),
        ],
        Cue::Defeat => vec![
            n(392, 0, 260, 50, Triangle),
            n(330, 260, 260, 50, Triangle),
            n(262, 520, 600, 50, Triangle),
            n(131, 520, 600, 25, Sine),
        ],
        Cue::Click => vec![n(1800, 0, 18, 25, Noise), n(1200, 0, 30, 20, Sine)],
    }
}

/// Volume steps for Player Settings, in percent.
pub fn volume_percent(step: u8) -> i32 {
    match step {
        0 => 0,
        1 => 35,
        2 => 70,
        _ => 100,
    }
}

/// The 16-bit samples of a cue at a volume (percent).
pub fn samples(cue: Cue, volume: i32) -> Vec<i16> {
    let ns = notes(cue);
    let end_ms = ns.iter().map(|n| n.at_ms + n.ms).max().unwrap_or(0) + 20;
    let len = (end_ms as u64 * SAMPLE_RATE as u64 / 1000) as usize;
    let mut mix = vec![0i32; len];
    let mut noise = Rng::new(cue as u64 + 1);
    for note in ns {
        let start = (note.at_ms as u64 * SAMPLE_RATE as u64 / 1000) as usize;
        let count = (note.ms as u64 * SAMPLE_RATE as u64 / 1000) as usize;
        // Phase in 1/65536 of a cycle, advanced by hz per sample.
        let step = (note.hz as u64 * 65_536 / SAMPLE_RATE as u64) as u32;
        let mut phase: u32 = 0;
        let attack = (SAMPLE_RATE as usize / 200).max(1); // 5 ms
        let release = (count / 3).max(1);
        for i in 0..count {
            let k = start + i;
            if k >= len {
                break;
            }
            phase = phase.wrapping_add(step) & 0xFFFF;
            let s: i32 = match note.wave {
                Wave::Sine => {
                    // 64-step table, linearly interpolated.
                    let pos = phase as usize * STEPS;
                    let a = sin64(pos >> 16);
                    let b = sin64((pos >> 16) + 1);
                    let f = (pos & 0xFFFF) as i32;
                    a + (((b - a) * f) >> 16)
                }
                Wave::Square => {
                    if phase < 0x8000 {
                        TRIG_ONE
                    } else {
                        -TRIG_ONE
                    }
                }
                Wave::Triangle => {
                    let p = phase as i32;
                    let t = if p < 0x8000 { p } else { 0xFFFF - p };
                    ((t * 4 * TRIG_ONE) >> 16) - TRIG_ONE
                }
                Wave::Noise => (noise.below(2 * TRIG_ONE as u32 + 1) as i32) - TRIG_ONE,
            };
            // Envelope in 1/1024: 5 ms attack, linear release over the last third.
            let level = if i < attack {
                (i * 1024 / attack) as i32
            } else if i + release > count {
                ((count - i) * 1024 / release) as i32
            } else {
                1024
            };
            mix[k] += s * level / 1024 * note.loud / 100;
        }
    }
    // Scale the mix (TRIG_ONE is full scale) into 16 bits, with headroom.
    mix.into_iter()
        .map(|v| {
            let v = v as i64 * 26_000 / TRIG_ONE as i64 * volume as i64 / 100;
            v.clamp(i16::MIN as i64, i16::MAX as i64) as i16
        })
        .collect()
}

/// A complete WAV file (RIFF, 16-bit mono PCM) for a cue.
pub fn wav(cue: Cue, volume: i32) -> Vec<u8> {
    let pcm = samples(cue, volume);
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_is_a_valid_wav_file() {
        for c in Cue::ALL {
            let w = wav(c, 70);
            assert_eq!(&w[0..4], b"RIFF", "{c:?}");
            assert_eq!(&w[8..16], b"WAVEfmt ", "{c:?}");
            let data_len = u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize;
            assert_eq!(w.len(), 44 + data_len, "{c:?}");
            assert!(data_len > 0);
            assert_eq!(Cue::from_key(c.key()), Some(c));
        }
    }

    #[test]
    fn sounds_are_audible_never_clip_and_scale_with_volume() {
        for c in Cue::ALL {
            let loud = samples(c, 100);
            let peak = loud.iter().map(|s| (*s as i32).abs()).max().unwrap();
            assert!(peak > 3_000, "{c:?} is audible: {peak}");
            assert!(peak < i16::MAX as i32, "{c:?} never clips: {peak}");
            let quiet = samples(c, 35);
            let qpeak = quiet.iter().map(|s| (*s as i32).abs()).max().unwrap();
            assert!(qpeak < peak, "{c:?} is quieter at 35%");
            assert!(
                samples(c, 0).iter().all(|s| *s == 0),
                "{c:?} is silent at 0"
            );
        }
    }

    #[test]
    fn a_warning_is_two_beeps() {
        // Silence between the two notes (110 ms on, 40 ms gap, then 160 ms).
        let s = samples(Cue::Warning, 100);
        let gap = (125 * SAMPLE_RATE / 1000) as usize;
        assert_eq!(s[gap], 0);
        let first = (50 * SAMPLE_RATE / 1000) as usize;
        assert!(s[first..first + 40].iter().any(|v| *v != 0));
    }
}
