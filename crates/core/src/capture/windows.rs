use super::watch::{build_input, spawn_watched};
use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Áudio do sistema no Windows: loopback WASAPI da saída padrão (o que os participantes
/// falam, tocando no fone ou no alto-falante). Trocar de saída reabre o loopback na nova.
pub struct SystemSource { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl SystemSource {
    pub fn new() -> Self { Self { stop: Arc::new(AtomicBool::new(false)), thread: None } }
}

impl Default for SystemSource { fn default() -> Self { Self::new() } }

fn default_name() -> Option<String> {
    cpal::default_host().default_output_device().and_then(|d| d.name().ok())
}

/// No WASAPI, uma stream de *entrada* aberta num dispositivo de *saída* é o loopback.
/// Enquanto nada toca, o loopback não entrega pedaços; o `SourceClock` completa com silêncio.
fn open(tx: Sender<CaptureEvent>, failed: Arc<AtomicBool>) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_output_device().context("nenhuma saída de áudio")?;
    let cfg = device.default_output_config()?;
    let conv = Mutex::new(Converter::new(cfg.sample_rate().0, cfg.channels() as usize));
    let emit = move |data: &[f32]| {
        let samples = conv.lock().unwrap().process(data);
        let _ = tx.send(CaptureEvent::Audio { source: SpeakerSource::System, samples });
    };
    let stream = build_input(&device, &cfg, emit, failed, "áudio do sistema")?;
    stream.play()?;
    Ok(stream)
}

impl AudioSource for SystemSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        self.stop();
        self.stop.store(false, Ordering::SeqCst);
        self.thread = Some(spawn_watched("system", SpeakerSource::System, self.stop.clone(), tx,
                                         default_name, open)?);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}

impl Drop for SystemSource {
    fn drop(&mut self) { self.stop(); }
}
