use std::collections::VecDeque;

pub trait VoiceDetector: Send {
    fn is_speech(&mut self, frame: &[f32]) -> bool;
}

/// Detector por energia. Substituível pelo Silero sem mudar o `Segmenter`.
pub struct RmsDetector { pub threshold: f32 }

impl VoiceDetector for RmsDetector {
    fn is_speech(&mut self, frame: &[f32]) -> bool {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt();
        rms > self.threshold
    }
}

#[derive(Debug, Clone)]
pub struct SegmenterConfig {
    pub frame_ms: u64,
    pub preroll_ms: u64,
    pub partial_every_ms: u64,
    pub silence_close_ms: u64,
    pub min_speech_ms: u64,
    pub max_utterance_ms: u64,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self { frame_ms: 30, preroll_ms: 200, partial_every_ms: 1_000,
               silence_close_ms: 600, min_speech_ms: 300, max_utterance_ms: 25_000 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEvent {
    Partial { seq: u64, start_ms: u64, audio: Vec<f32> },
    Final { seq: u64, start_ms: u64, end_ms: u64, audio: Vec<f32> },
}

pub struct Segmenter {
    cfg: SegmenterConfig,
    vad: Box<dyn VoiceDetector>,
    frame_len: usize,
    pending: Vec<f32>,
    preroll: VecDeque<Vec<f32>>,
    clock_ms: u64,
    active: bool,
    buf: Vec<f32>,
    start_ms: u64,
    speech_ms: u64,
    silence_ms: u64,
    since_partial_ms: u64,
    seq: u64,
}

impl Segmenter {
    pub fn new(cfg: SegmenterConfig, vad: Box<dyn VoiceDetector>) -> Self {
        let frame_len = (cfg.frame_ms * 16) as usize;
        Self { cfg, vad, frame_len, pending: vec![], preroll: VecDeque::new(), clock_ms: 0,
               active: false, buf: vec![], start_ms: 0, speech_ms: 0, silence_ms: 0,
               since_partial_ms: 0, seq: 0 }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<SegmentEvent> {
        self.pending.extend_from_slice(samples);
        let mut out = vec![];
        while self.pending.len() >= self.frame_len {
            let frame: Vec<f32> = self.pending.drain(..self.frame_len).collect();
            self.step(frame, &mut out);
        }
        out
    }

    pub fn flush(&mut self) -> Vec<SegmentEvent> {
        let mut out = vec![];
        if self.active { self.close(&mut out); }
        out
    }

    fn step(&mut self, frame: Vec<f32>, out: &mut Vec<SegmentEvent>) {
        let fm = self.cfg.frame_ms;
        let speech = self.vad.is_speech(&frame);
        self.clock_ms += fm;
        // Corte forçado: se o frame estouraria o limite, fecha antes dele;
        // o frame então abre a próxima fala (ou vai ao pré-roll).
        if self.active && (self.buf.len() + frame.len()) as u64 / 16 > self.cfg.max_utterance_ms {
            self.close(out);
        }
        if !self.active {
            if speech {
                self.active = true;
                let pre: Vec<f32> = self.preroll.drain(..).flatten().collect();
                self.start_ms = self.clock_ms - fm - (pre.len() as u64 / 16);
                self.buf = pre;
                self.buf.extend_from_slice(&frame);
                self.speech_ms = fm;
                self.silence_ms = 0;
                self.since_partial_ms = 0;
            } else {
                self.preroll.push_back(frame);
                while self.preroll.len() as u64 * fm > self.cfg.preroll_ms { self.preroll.pop_front(); }
            }
            return;
        }
        self.buf.extend_from_slice(&frame);
        self.since_partial_ms += fm;
        if speech { self.speech_ms += fm; self.silence_ms = 0; } else { self.silence_ms += fm; }
        if self.silence_ms >= self.cfg.silence_close_ms {
            self.close(out);
        } else if self.since_partial_ms >= self.cfg.partial_every_ms {
            self.since_partial_ms = 0;
            out.push(SegmentEvent::Partial { seq: self.seq, start_ms: self.start_ms, audio: self.buf.clone() });
        }
    }

    fn close(&mut self, out: &mut Vec<SegmentEvent>) {
        self.active = false;
        let audio = std::mem::take(&mut self.buf);
        if self.speech_ms >= self.cfg.min_speech_ms {
            let end_ms = self.start_ms + audio.len() as u64 / 16;
            out.push(SegmentEvent::Final { seq: self.seq, start_ms: self.start_ms, end_ms, audio });
            self.seq += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: usize = 16; // amostras por ms a 16 kHz

    fn tone(ms: usize) -> Vec<f32> {
        (0..ms * MS).map(|i| 0.3 * (i as f32 * 0.17).sin()).collect()
    }
    fn silence(ms: usize) -> Vec<f32> { vec![0.0; ms * MS] }

    fn seg() -> Segmenter {
        Segmenter::new(SegmenterConfig::default(), Box::new(RmsDetector { threshold: 0.01 }))
    }

    fn finals(ev: &[SegmentEvent]) -> Vec<(u64, u64, u64)> {
        ev.iter().filter_map(|e| match e {
            SegmentEvent::Final { seq, start_ms, end_ms, .. } => Some((*seq, *start_ms, *end_ms)),
            _ => None,
        }).collect()
    }

    #[test]
    fn silence_produces_nothing() {
        let mut s = seg();
        assert!(s.push(&silence(5_000)).is_empty());
        assert!(s.flush().is_empty());
    }

    #[test]
    fn speech_then_pause_closes_one_utterance() {
        let mut s = seg();
        let mut ev = s.push(&silence(500));
        ev.extend(s.push(&tone(1_500)));
        ev.extend(s.push(&silence(1_000)));
        let f = finals(&ev);
        assert_eq!(f.len(), 1);
        let (seq, start, end) = f[0];
        assert_eq!(seq, 0);
        assert!((280..=500).contains(&start), "start = {start}"); // 200 ms de pré-roll
        assert!((2_000..=2_700).contains(&end), "end = {end}");
    }

    #[test]
    fn long_speech_emits_partials_with_same_seq() {
        let mut s = seg();
        let ev = s.push(&tone(3_500));
        let partials: Vec<u64> = ev.iter().filter_map(|e| match e {
            SegmentEvent::Partial { seq, .. } => Some(*seq), _ => None }).collect();
        assert!(partials.len() >= 3, "parciais = {}", partials.len());
        assert!(partials.iter().all(|&q| q == 0));
    }

    #[test]
    fn monologue_is_force_closed_before_whisper_limit() {
        let mut s = seg();
        let ev = s.push(&tone(40_000));
        let f = finals(&ev);
        assert_eq!(f.len(), 1);
        assert!(f[0].2 - f[0].1 <= 25_000);
        let rest = finals(&s.flush());
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].0, 1);
    }

    #[test]
    fn short_click_is_discarded() {
        let mut s = seg();
        let mut ev = s.push(&tone(60));
        ev.extend(s.push(&silence(1_000)));
        assert!(finals(&ev).is_empty());
    }

    #[test]
    fn odd_chunk_sizes_do_not_lose_samples() {
        let mut s = seg();
        let audio: Vec<f32> = [tone(1_000), silence(1_000)].concat();
        let mut ev = vec![];
        for c in audio.chunks(333) { ev.extend(s.push(c)); }
        assert_eq!(finals(&ev).len(), 1);
    }
}
