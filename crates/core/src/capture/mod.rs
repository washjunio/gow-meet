use crate::resample::{to_mono, Resampler, SAMPLE_RATE};
use crate::session::SpeakerSource;
use anyhow::Result;
use crossbeam_channel::Sender;

pub mod mic;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "macos")]
pub use mac::SystemSource;
#[cfg(not(target_os = "macos"))]
pub use unsupported::SystemSource;
pub use mic::MicSource;

/// Mantém o CI do Windows compilando até a Task 14 implementar o loopback WASAPI.
#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::{AudioSource, CaptureEvent};
    use anyhow::Result;
    use crossbeam_channel::Sender;
    #[derive(Default)]
    pub struct SystemSource;
    impl SystemSource { pub fn new() -> Self { Self } }
    impl AudioSource for SystemSource {
        fn start(&mut self, _tx: Sender<CaptureEvent>) -> Result<()> {
            anyhow::bail!("captura do áudio do sistema ainda não disponível nesta plataforma")
        }
        fn stop(&mut self) {}
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureEvent {
    Audio { source: SpeakerSource, samples: Vec<f32> },
    Reconnecting(SpeakerSource),
    Error { source: SpeakerSource, message: String },
}

pub trait AudioSource: Send {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()>;
    fn stop(&mut self);
}

pub struct Converter { channels: usize, resampler: Option<Resampler> }

impl Converter {
    pub fn new(in_rate: u32, channels: usize) -> Self {
        let resampler = (in_rate != SAMPLE_RATE).then(|| Resampler::new(in_rate, SAMPLE_RATE));
        Self { channels, resampler }
    }
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let mono = to_mono(interleaved, self.channels);
        match &mut self.resampler { Some(r) => r.process(&mono), None => mono }
    }
}

#[cfg(test)]
mod tests {
    use super::Converter;
    #[test]
    fn stereo_48k_becomes_mono_16k() {
        let mut c = Converter::new(48_000, 2);
        let stereo: Vec<f32> = (0..48_000).flat_map(|_| [0.5f32, 0.5]).collect();
        let out = c.process(&stereo);
        assert!((out.len() as i64 - 16_000).abs() <= 2);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-3));
    }
    #[test]
    fn mono_16k_passes_through() {
        let mut c = Converter::new(16_000, 1);
        assert_eq!(c.process(&[0.1, 0.2, 0.3]), vec![0.1, 0.2, 0.3]);
    }
}
