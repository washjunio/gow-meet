pub mod asr;
pub mod capture;
pub mod fbank;
pub mod lang;
pub mod models;
pub mod pipeline;
pub mod resample;
pub mod segmenter;
pub mod session;
pub mod speaker;
pub mod tracker;
pub mod translate;
pub mod voices;
pub mod wav;
pub mod wer;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert_eq!(super::version(), "0.1.0");
    }
}
