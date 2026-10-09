use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct MicSource { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>> }

impl MicSource {
    pub fn new() -> Self { Self { stop: Arc::new(AtomicBool::new(false)), thread: None } }
}

impl Default for MicSource { fn default() -> Self { Self::new() } }

fn default_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| d.name().ok())
}

fn open(tx: Sender<CaptureEvent>, failed: Arc<AtomicBool>) -> Result<cpal::Stream> {
    let device = cpal::default_host().default_input_device().context("nenhum microfone")?;
    let cfg = device.default_input_config()?;
    let conv = Mutex::new(Converter::new(cfg.sample_rate().0, cfg.channels() as usize));
    let emit = move |data: &[f32]| {
        let samples = conv.lock().unwrap().process(data);
        let _ = tx.send(CaptureEvent::Audio { source: SpeakerSource::Mic, samples });
    };
    let on_err = move |_err| failed.store(true, Ordering::SeqCst);
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &cfg.config(), move |data: &[f32], _| emit(data), on_err, None)?,
        SampleFormat::I16 => device.build_input_stream(
            &cfg.config(),
            move |data: &[i16], _| {
                let f: Vec<f32> = data.iter().map(|&v| v as f32 / 32768.0).collect();
                emit(&f)
            },
            on_err, None)?,
        other => bail!("formato de microfone não suportado: {other:?}"),
    };
    stream.play()?;
    Ok(stream)
}

impl AudioSource for MicSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        self.stop();
        let stop = self.stop.clone();
        stop.store(false, Ordering::SeqCst);
        self.thread = Some(std::thread::Builder::new().name("mic".into()).spawn(move || {
            // O `cpal::Stream` não é `Send` no Mac: nasce, vive e morre nesta thread.
            while !stop.load(Ordering::SeqCst) {
                let failed = Arc::new(AtomicBool::new(false));
                let name = default_name();
                let stream = match open(tx.clone(), failed.clone()) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(CaptureEvent::Error { source: SpeakerSource::Mic, message: e.to_string() });
                        sleep_unless_stopped(&stop, Duration::from_secs(2));
                        continue;
                    }
                };
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(200));
                    if failed.load(Ordering::SeqCst) || default_name() != name {
                        let _ = tx.send(CaptureEvent::Reconnecting(SpeakerSource::Mic));
                        break;
                    }
                }
                drop(stream);
            }
        })?);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() { let _ = h.join(); }
    }
}

/// Dorme em fatias curtas para que `stop()` não espere os 2 s inteiros do retry.
fn sleep_unless_stopped(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(100);
    let mut left = total;
    while !left.is_zero() && !stop.load(Ordering::SeqCst) {
        let d = step.min(left);
        std::thread::sleep(d);
        left -= d;
    }
}

impl Drop for MicSource {
    fn drop(&mut self) { self.stop(); }
}
