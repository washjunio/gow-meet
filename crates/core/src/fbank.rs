//! Filterbank log-mel no formato Kaldi, como o WeSpeaker usa na inferência:
//! amostras em escala int16, janela de 25 ms a cada 10 ms (snip_edges), DC removido,
//! pré-ênfase 0,97, janela de Hamming, FFT de 512, potência, 80 filtros mel de 20 Hz a
//! 8 kHz, log com piso em f32::EPSILON. `cmn` subtrai a média de cada filtro no tempo.
use rustfft::{num_complex::Complex32, FftPlanner};
use std::sync::Arc;

pub const NUM_BINS: usize = 80;
const FRAME_LEN: usize = 400;
const FRAME_SHIFT: usize = 160;
const FFT_LEN: usize = 512;
const PREEMPH: f32 = 0.97;
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 8_000.0;

fn mel(hz: f32) -> f32 { 1127.0 * (1.0 + hz / 700.0).ln() }

pub struct Fbank {
    window: Vec<f32>,
    /// Por filtro: primeiro índice da FFT e pesos.
    filters: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl Default for Fbank {
    fn default() -> Self { Self::new() }
}

impl Fbank {
    pub fn new() -> Self {
        let window = (0..FRAME_LEN)
            .map(|i| 0.54 - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / (FRAME_LEN - 1) as f32).cos())
            .collect();
        let (lo, hi) = (mel(LOW_HZ), mel(HIGH_HZ));
        let delta = (hi - lo) / (NUM_BINS + 1) as f32;
        let bin_hz = 16_000.0 / FFT_LEN as f32;
        let filters = (0..NUM_BINS).map(|b| {
            let (left, center, right) = (lo + b as f32 * delta, lo + (b + 1) as f32 * delta, lo + (b + 2) as f32 * delta);
            let mut first = None;
            let mut w = vec![];
            for i in 0..FFT_LEN / 2 {
                let m = mel(bin_hz * i as f32);
                if m > left && m < right {
                    first.get_or_insert(i);
                    w.push(if m <= center { (m - left) / (center - left) } else { (right - m) / (right - center) });
                }
            }
            (first.unwrap_or(0), w)
        }).collect();
        Self { window, filters, fft: FftPlanner::new().plan_fft_forward(FFT_LEN) }
    }

    /// `audio`: 16 kHz mono em [-1, 1]. Devolve `frames × NUM_BINS` (linha a linha).
    pub fn compute(&self, audio: &[f32]) -> Vec<[f32; NUM_BINS]> {
        if audio.len() < FRAME_LEN { return vec![]; }
        let n = 1 + (audio.len() - FRAME_LEN) / FRAME_SHIFT;
        let mut out = Vec::with_capacity(n);
        let mut frame = [0f32; FRAME_LEN];
        let mut buf = vec![Complex32::new(0.0, 0.0); FFT_LEN];
        for t in 0..n {
            let src = &audio[t * FRAME_SHIFT..t * FRAME_SHIFT + FRAME_LEN];
            for (d, s) in frame.iter_mut().zip(src) { *d = s * 32_768.0; }
            let mean = frame.iter().sum::<f32>() / FRAME_LEN as f32;
            for s in frame.iter_mut() { *s -= mean; }
            for i in (1..FRAME_LEN).rev() { frame[i] -= PREEMPH * frame[i - 1]; }
            frame[0] -= PREEMPH * frame[0];
            for (i, c) in buf.iter_mut().enumerate() {
                *c = Complex32::new(if i < FRAME_LEN { frame[i] * self.window[i] } else { 0.0 }, 0.0);
            }
            self.fft.process(&mut buf);
            let power: Vec<f32> = buf[..FFT_LEN / 2].iter().map(|c| c.norm_sqr()).collect();
            let mut row = [0f32; NUM_BINS];
            for (r, (first, w)) in row.iter_mut().zip(&self.filters) {
                let e: f32 = w.iter().zip(&power[*first..]).map(|(a, p)| a * p).sum();
                *r = e.max(f32::EPSILON).ln();
            }
            out.push(row);
        }
        out
    }
}

/// Normalização de média por filtro (CMN), como no WeSpeaker.
pub fn cmn(feats: &mut [[f32; NUM_BINS]]) {
    if feats.is_empty() { return; }
    let mut mean = [0f32; NUM_BINS];
    for row in feats.iter() { for (m, v) in mean.iter_mut().zip(row) { *m += v; } }
    for m in mean.iter_mut() { *m /= feats.len() as f32; }
    for row in feats.iter_mut() { for (v, m) in row.iter_mut().zip(&mean) { *v -= m; } }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Referência: kaldi-native-fbank 1.20.0 (dither 0, janela hamming, 80 filtros) sobre
    /// o mesmo sinal, em escala int16.
    #[test]
    fn matches_kaldi_native_fbank() {
        let x: Vec<f32> = (0..16_000).map(|n| {
            let t = n as f32 / 16_000.0;
            (8_000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 3_000.0 * (2.0 * std::f32::consts::PI * 1_234.0 * t).sin()) / 32_768.0
        }).collect();
        let f = Fbank::new().compute(&x);
        assert_eq!(f.len(), 98);
        for (t, b, want) in [(0, 0, 11.671196), (0, 10, 11.286946), (0, 40, 13.254454),
                             (50, 5, 11.499412), (50, 79, 11.802671), (97, 20, 12.014282)] {
            assert!((f[t][b] - want).abs() < 0.02, "f[{t}][{b}] = {} (esperado {want})", f[t][b]);
        }
        let s: f32 = f[0].iter().sum();
        assert!((s - 1084.5693).abs() < 0.5, "soma = {s}");
    }

    #[test]
    fn short_audio_gives_no_frames_and_cmn_zeroes_mean() {
        assert!(Fbank::new().compute(&[0.1; 399]).is_empty());
        let mut f = vec![[1.0; NUM_BINS], [3.0; NUM_BINS]];
        cmn(&mut f);
        assert_eq!(f[0][0], -1.0);
        assert_eq!(f[1][79], 1.0);
    }
}
