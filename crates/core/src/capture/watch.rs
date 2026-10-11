use super::CaptureEvent;
use crate::session::SpeakerSource;
use anyhow::{bail, Result};
use cpal::traits::DeviceTrait;
use cpal::SampleFormat;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Mantém uma stream cpal viva numa thread própria e a recria quando o dispositivo
/// padrão muda (`default_name` diferente) ou a stream reporta erro (`failed`).
/// `open` recebe o canal e a flag de falha que o callback de erro deve levantar.
pub fn spawn_watched<O>(
    name: &str, source: SpeakerSource, stop: Arc<AtomicBool>, tx: Sender<CaptureEvent>,
    default_name: fn() -> Option<String>, mut open: O,
) -> std::io::Result<JoinHandle<()>>
where
    O: FnMut(Sender<CaptureEvent>, Arc<AtomicBool>) -> Result<cpal::Stream> + Send + 'static,
{
    std::thread::Builder::new().name(name.into()).spawn(move || {
        // O `cpal::Stream` não é `Send` no Mac: nasce, vive e morre nesta thread.
        while !stop.load(Ordering::SeqCst) {
            let failed = Arc::new(AtomicBool::new(false));
            let device = default_name();
            let stream = match open(tx.clone(), failed.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(CaptureEvent::Error { source, message: e.to_string() });
                    sleep_unless_stopped(&stop, Duration::from_secs(2));
                    continue;
                }
            };
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(200));
                if failed.load(Ordering::SeqCst) || default_name() != device {
                    let _ = tx.send(CaptureEvent::Reconnecting(source));
                    break;
                }
            }
            drop(stream);
        }
    })
}

/// Abre uma stream de entrada em `f32` ou `i16` (convertido para `f32`) e entrega cada
/// pedaço intercalado a `emit`. Erros da stream levantam `failed`.
pub fn build_input(
    device: &cpal::Device, cfg: &cpal::SupportedStreamConfig, mut emit: impl FnMut(&[f32]) + Send + 'static,
    failed: Arc<AtomicBool>, what: &str,
) -> Result<cpal::Stream> {
    let on_err = move |_err| failed.store(true, Ordering::SeqCst);
    Ok(match cfg.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &cfg.config(), move |data: &[f32], _| emit(data), on_err, None)?,
        SampleFormat::I16 => device.build_input_stream(
            &cfg.config(),
            move |data: &[i16], _| {
                let f: Vec<f32> = data.iter().map(|&v| v as f32 / 32768.0).collect();
                emit(&f)
            },
            on_err, None)?,
        other => bail!("formato de {what} não suportado: {other:?}"),
    })
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
