use crate::resample::{to_mono, Resampler, SAMPLE_RATE};
use anyhow::Result;
use std::path::Path;

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / max)).collect::<Result<_, _>>()?
        }
    };
    let mono = to_mono(&samples, spec.channels as usize);
    if spec.sample_rate == SAMPLE_RATE { return Ok(mono); }
    Ok(Resampler::new(spec.sample_rate, SAMPLE_RATE).process(&mono))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_fixture_at_16k() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/s1.wav");
        let audio = super::read_wav_16k_mono(&p).unwrap();
        assert!(audio.len() > 16_000, "len = {}", audio.len());
        assert!(audio.iter().any(|s| s.abs() > 0.05));
    }
}
