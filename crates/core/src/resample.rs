pub const SAMPLE_RATE: u32 = 16_000;

pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 { return interleaved.to_vec(); }
    interleaved.chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Interpolação linear em fluxo contínuo. Suficiente para voz; não aplica
/// filtro passa-baixa (a fala útil fica abaixo de 8 kHz).
pub struct Resampler { step: f64, pos: f64, last: Option<f32> }

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self { step: in_rate as f64 / out_rate as f64, pos: 0.0, last: None }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() { return vec![]; }
        let mut buf = Vec::with_capacity(input.len() + 1);
        if let Some(l) = self.last { buf.push(l); }
        buf.extend_from_slice(input);
        let end = (buf.len() - 1) as f64;
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        let mut t = self.pos;
        while t <= end {
            let i = t.floor() as usize;
            let f = (t - i as f64) as f32;
            let b = if i + 1 < buf.len() { buf[i + 1] } else { buf[i] };
            out.push(buf[i] * (1.0 - f) + b * f);
            t += self.step;
        }
        self.pos = t - end;
        self.last = buf.last().copied();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin())
            .collect()
    }

    #[test]
    fn downmix_stereo_averages_channels() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_48k_to_16k_in_chunks_keeps_duration() {
        let input = sine(48_000, 1.0);
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = vec![];
        for chunk in input.chunks(1_000) { out.extend(r.process(chunk)); }
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len = {}", out.len());
        assert!(out.iter().all(|s| s.abs() <= 1.0001));
    }

    #[test]
    fn resample_44100_to_16k_keeps_duration() {
        let mut r = Resampler::new(44_100, 16_000);
        let out = r.process(&sine(44_100, 2.0));
        assert!((out.len() as i64 - 32_000).abs() <= 2, "len = {}", out.len());
    }
}
