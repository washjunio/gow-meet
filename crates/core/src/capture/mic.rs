use super::watch::{build_input, spawn_watched};
use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub struct MicSource { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl MicSource {
    pub fn new() -> Self { Self { stop: Arc::new(AtomicBool::new(false)), thread: None } }
}

impl Default for MicSource { fn default() -> Self { Self::new() } }

fn default_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

/// Aviso quando o microfone só entrega zeros exatos: é assim que o macOS responde
/// quando o acesso ao Microfone foi negado. A interface reconhece "permissão" + "Microfone".
pub const MIC_SILENT_MESSAGE: &str =
    "O microfone só entrega silêncio absoluto: verifique a permissão de Microfone do GOW Meet.";

/// Amostras (16 kHz) de zeros exatos antes do aviso: ~3 s.
const SILENT_LIMIT: u64 = 3 * crate::resample::SAMPLE_RATE as u64;

/// Detecta um microfone que, desde o início, só entrega zeros exatos (acesso negado).
/// Dispara no máximo uma vez; qualquer amostra não nula o desarma de vez.
#[derive(Debug)]
pub struct ZeroWatch { zeros: u64, limit: u64, done: bool }

impl ZeroWatch {
    pub fn new(limit_samples: u64) -> Self { Self { zeros: 0, limit: limit_samples, done: false } }

    /// `true` exatamente uma vez: quando o total de zeros exatos atinge o limite.
    pub fn observe(&mut self, samples: &[f32]) -> bool {
        if self.done { return false; }
        if samples.iter().any(|&s| s != 0.0) {
            self.done = true;
            return false;
        }
        self.zeros += samples.len() as u64;
        if self.zeros >= self.limit {
            self.done = true;
            return true;
        }
        false
    }
}

fn open(tx: Sender<CaptureEvent>, failed: Arc<AtomicBool>, watch: Arc<Mutex<ZeroWatch>>)
    -> Result<cpal::Stream> {
    let device = cpal::default_host().default_input_device().context("nenhum microfone")?;
    let cfg = device.default_input_config()?;
    let conv = Mutex::new(Converter::new(cfg.sample_rate().0, cfg.channels() as usize));
    let emit = move |data: &[f32]| {
        let samples = conv.lock().unwrap().process(data);
        let silent = watch.lock().unwrap().observe(&samples);
        let _ = tx.send(CaptureEvent::Audio { source: SpeakerSource::Mic, samples });
        if silent {
            let _ = tx.send(CaptureEvent::Error { source: SpeakerSource::Mic,
                                                  message: MIC_SILENT_MESSAGE.into() });
        }
    };
    let stream = build_input(&device, &cfg, emit, failed, "microfone")?;
    stream.play()?;
    Ok(stream)
}

impl AudioSource for MicSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        self.stop();
        self.stop.store(false, Ordering::SeqCst);
        // Um vigia por início de captura, compartilhado pelas reconexões.
        let watch = Arc::new(Mutex::new(ZeroWatch::new(SILENT_LIMIT)));
        self.thread = Some(spawn_watched("mic", SpeakerSource::Mic, self.stop.clone(), tx, default_name,
            move |tx, failed| open(tx, failed, watch.clone()))?);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}

impl Drop for MicSource {
    fn drop(&mut self) { self.stop(); }
}

#[cfg(test)]
mod tests {
    use super::{ZeroWatch, MIC_SILENT_MESSAGE, SILENT_LIMIT};

    #[test]
    fn only_zeros_for_three_seconds_fires_once() {
        let mut w = ZeroWatch::new(SILENT_LIMIT);
        let chunk = vec![0.0f32; 160]; // 10 ms
        let fired: Vec<bool> = (0..400).map(|_| w.observe(&chunk)).collect();
        assert_eq!(fired.iter().filter(|&&f| f).count(), 1);
        assert_eq!(fired.iter().position(|&f| f), Some(299)); // 300 × 10 ms = 3 s
    }

    #[test]
    fn any_signal_disarms_for_good() {
        let mut w = ZeroWatch::new(SILENT_LIMIT);
        assert!(!w.observe(&vec![0.0; 16_000]));
        let mut quiet = vec![0.0f32; 160];
        quiet[3] = 1e-6; // ruído mínimo de um microfone real
        assert!(!w.observe(&quiet));
        assert!(!w.observe(&vec![0.0; 100_000]));
    }

    #[test]
    fn short_silence_does_not_fire() {
        let mut w = ZeroWatch::new(SILENT_LIMIT);
        assert!(!w.observe(&vec![0.0; 47_999]));
    }

    #[test]
    fn message_mentions_microphone_permission() {
        assert!(MIC_SILENT_MESSAGE.contains("permissão de Microfone"));
    }
}
