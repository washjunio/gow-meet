use super::{AudioSource, CaptureEvent, Converter};
use crate::session::SpeakerSource;
use anyhow::{Context, Result};
use core_media_rs::cm_sample_buffer::CMSampleBuffer;
use crossbeam_channel::Sender;
use screencapturekit::shareable_content::SCShareableContent;
use screencapturekit::stream::{
    configuration::SCStreamConfiguration, content_filter::SCContentFilter,
    output_trait::SCStreamOutputTrait, output_type::SCStreamOutputType, SCStream,
};
use std::sync::Mutex;

const RATE: u32 = 48_000;

/// `CFError` não é `Send + Sync`, então não converte direto em `anyhow::Error`.
fn cf_err<E: std::fmt::Debug>(e: E) -> anyhow::Error { anyhow::anyhow!("configurar captura: {e:?}") }

struct Handler { tx: Sender<CaptureEvent>, conv: Mutex<Converter> }

impl SCStreamOutputTrait for Handler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Audio { return; }
        let Ok(list) = sample.get_audio_buffer_list() else { return };
        // A captura é pedida em mono, então há um único buffer; se vierem vários
        // (não intercalados), faz a média para não duplicar a linha do tempo.
        let n = list.num_buffers();
        let mut mix: Vec<f32> = Vec::new();
        for i in 0..n {
            let Some(buf) = list.get(i) else { continue };
            let data = buf.data().chunks_exact(4)
                .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]));
            if i == 0 { mix.extend(data); } else {
                for (m, s) in mix.iter_mut().zip(data) { *m += s; }
            }
        }
        if n > 1 { for m in &mut mix { *m /= n as f32; } }
        if mix.is_empty() { return; }
        let samples = self.conv.lock().unwrap().process(&mix);
        let _ = self.tx.send(CaptureEvent::Audio { source: SpeakerSource::System, samples });
    }
}

/// Captura a mistura de áudio do sistema via ScreenCaptureKit.
///
/// `SCStream` já é `Send` na crate `screencapturekit` (impl dela, sobre um objeto
/// ScreenCaptureKit que aceita start/stop de qualquer thread), então este tipo é `Send`
/// automaticamente, sem `unsafe` aqui. `start` e `stop` exigem `&mut self`, o que já
/// impede uso concorrente da mesma instância.
pub struct SystemSource { stream: Option<SCStream> }

impl SystemSource {
    pub fn new() -> Self { Self { stream: None } }
}

impl Default for SystemSource { fn default() -> Self { Self::new() } }

impl AudioSource for SystemSource {
    fn start(&mut self, tx: Sender<CaptureEvent>) -> Result<()> {
        self.stop();
        let content = SCShareableContent::get().map_err(|e| anyhow::anyhow!(
            "sem permissão de Gravação de Tela e Áudio do Sistema ({e:?})"))?;
        let display = content.displays().into_iter().next().context("nenhuma tela encontrada")?;
        let filter = SCContentFilter::new().with_display_excluding_windows(&display, &[]);
        let config = SCStreamConfiguration::new()
            .set_captures_audio(true).map_err(cf_err)?
            .set_sample_rate(RATE).map_err(cf_err)?
            .set_channel_count(1).map_err(cf_err)?
            .set_excludes_current_process_audio(true).map_err(cf_err)?
            .set_width(2).map_err(cf_err)?
            .set_height(2).map_err(cf_err)?;
        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(Handler { tx, conv: Mutex::new(Converter::new(RATE, 1)) },
                                  SCStreamOutputType::Audio);
        stream.start_capture().map_err(|e| anyhow::anyhow!("iniciar captura: {e:?}"))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(s) = self.stream.take() { let _ = s.stop_capture(); }
    }
}

/// `SCStream::drop` libera os handlers sem chamar stopCapture; parar antes evita que o
/// ScreenCaptureKit chame memória liberada. `stop` é idempotente.
impl Drop for SystemSource {
    fn drop(&mut self) { self.stop(); }
}
