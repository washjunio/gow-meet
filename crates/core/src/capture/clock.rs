use crate::resample::SAMPLE_RATE;
use crate::session::SpeakerSource;
use std::collections::HashMap;
use std::time::Duration;

/// Atraso tolerado antes de completar uma fonte com silêncio.
pub const DEFAULT_MAX_BEHIND_MS: u64 = 300;

/// Atraso tolerado no `fill_to` periódico: maior que o do caminho por pedaço, para
/// absorver o jitter de entrega sem preencher à toa.
pub const DEFAULT_TICK_BEHIND_MS: u64 = 400;

const PER_MS: u64 = SAMPLE_RATE as u64 / 1_000;
/// Zeros são entregues em pedaços de 1 s para não alocar lacunas longas de uma vez.
static ZERO_CHUNK: [f32; SAMPLE_RATE as usize] = [0.0; SAMPLE_RATE as usize];

/// Mantém a linha do tempo de cada fonte presa ao relógio de parede.
///
/// O t0 das falas é "amostras entregues" de cada fonte; quando uma fonte para de
/// entregar (reconexão do microfone, troca de dispositivo, ScreenCaptureKit calado
/// no silêncio), ela fica para trás e suas falas ganham t0 antigos. Antes de cada
/// pedaço novo, se a fonte está mais de `max_behind_ms` atrás do tempo decorrido
/// desde o início da sessão, a lacuna é preenchida com zeros.
pub struct SourceClock {
    fed: HashMap<SpeakerSource, u64>,
    max_behind: u64,
    tick_behind: u64,
}

impl SourceClock {
    pub fn new(max_behind_ms: u64) -> Self {
        Self { fed: HashMap::new(), max_behind: max_behind_ms * PER_MS,
               tick_behind: DEFAULT_TICK_BEHIND_MS * PER_MS }
    }

    /// Quantos zeros entregar antes de um pedaço de `incoming` amostras que chegou
    /// `elapsed` depois do início da sessão. Nunca negativo: uma fonte adiantada
    /// (pedaços entregues em rajada) não recebe nada.
    pub fn gap_before(&mut self, source: SpeakerSource, elapsed: Duration, incoming: usize) -> u64 {
        let expected = (elapsed.as_micros() as u64).saturating_mul(PER_MS) / 1_000;
        let fed = self.fed.entry(source).or_insert(0);
        let after = *fed + incoming as u64;
        let gap = expected.saturating_sub(after);
        let fill = if gap > self.max_behind { gap } else { 0 };
        *fed = after + fill;
        fill
    }

    /// Entrega a `feed` os zeros que faltam (em pedaços de até 1 s) e depois `samples`.
    pub fn align(&mut self, source: SpeakerSource, elapsed: Duration, samples: &[f32],
                 mut feed: impl FnMut(&[f32])) {
        let mut gap = self.gap_before(source, elapsed, samples.len());
        while gap > 0 {
            let n = gap.min(ZERO_CHUNK.len() as u64) as usize;
            feed(&ZERO_CHUNK[..n]);
            gap -= n as u64;
        }
        feed(samples);
    }

    /// Completa com silêncio as fontes que já entregaram e estão mais de 400 ms atrás de
    /// `elapsed`, sem esperar o próximo pedaço. O ScreenCaptureKit não entrega nada enquanto
    /// não toca áudio, e sem esse silêncio o segmentador nunca fecha a última fala.
    /// Fontes que nunca entregaram não recebem nada. `feed` recebe a fonte e os zeros
    /// (em pedaços de até 1 s).
    pub fn fill_to(&mut self, elapsed: Duration, mut feed: impl FnMut(SpeakerSource, &[f32])) {
        let expected = (elapsed.as_micros() as u64).saturating_mul(PER_MS) / 1_000;
        for (&source, fed) in self.fed.iter_mut() {
            let mut gap = expected.saturating_sub(*fed);
            if gap <= self.tick_behind { continue; }
            *fed += gap;
            while gap > 0 {
                let n = gap.min(ZERO_CHUNK.len() as u64) as usize;
                feed(source, &ZERO_CHUNK[..n]);
                gap -= n as u64;
            }
        }
    }
}

impl Default for SourceClock {
    fn default() -> Self { Self::new(DEFAULT_MAX_BEHIND_MS) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SpeakerSource::*;

    fn ms(n: u64) -> Duration { Duration::from_millis(n) }

    /// Alimenta pela `align` e devolve (zeros entregues, total entregue).
    fn feed(c: &mut SourceClock, s: SpeakerSource, at_ms: u64, n: usize) -> (usize, usize) {
        let chunk = vec![0.5f32; n];
        let (mut zeros, mut total) = (0, 0);
        c.align(s, ms(at_ms), &chunk, |x| {
            total += x.len();
            if x.iter().all(|&v| v == 0.0) && !x.is_empty() { zeros += x.len(); }
        });
        (zeros, total)
    }

    #[test]
    fn on_time_source_gets_no_fill() {
        let mut c = SourceClock::default();
        // pedaços de 100 ms chegando no fim de cada janela de 100 ms
        for i in 1..=50u64 {
            assert_eq!(feed(&mut c, Mic, i * 100, 1_600), (0, 1_600));
        }
        // atraso dentro da tolerância (200 ms) também não é preenchido
        assert_eq!(feed(&mut c, Mic, 5_300, 1_600), (0, 1_600));
    }

    #[test]
    fn gap_is_filled_up_to_wall_clock() {
        let mut c = SourceClock::default();
        assert_eq!(feed(&mut c, System, 100, 1_600), (0, 1_600));
        // fonte calada por ~10 s: o próximo pedaço (100 ms) chega em 10,1 s
        let (zeros, total) = feed(&mut c, System, 10_100, 1_600);
        assert_eq!(zeros, 10_000 * 16 - 1_600);
        assert_eq!(total, 10_000 * 16); // fonte volta a 10,1 s de linha do tempo
        // de volta ao ritmo: sem novos zeros
        assert_eq!(feed(&mut c, System, 10_200, 1_600), (0, 1_600));
    }

    #[test]
    fn sources_are_tracked_independently() {
        let mut c = SourceClock::default();
        assert_eq!(feed(&mut c, Mic, 2_000, 32_000), (0, 32_000));
        let (zeros, _) = feed(&mut c, System, 2_000, 1_600);
        assert_eq!(zeros, 32_000 - 1_600);
    }

    #[test]
    fn source_ahead_of_clock_never_goes_backwards() {
        let mut c = SourceClock::default();
        // rajada: 5 s de áudio entregues em 1 s de relógio
        assert_eq!(feed(&mut c, Mic, 1_000, 80_000), (0, 80_000));
        assert_eq!(c.gap_before(Mic, ms(2_000), 1_600), 0);
        assert_eq!(c.gap_before(Mic, ms(0), 0), 0);
        // só volta a preencher quando o relógio passa a fonte de novo
        assert_eq!(c.gap_before(Mic, ms(10_000), 0), 10_000 * 16 - 81_600);
    }

    /// Soma de zeros entregues por `fill_to`, por fonte.
    fn fill(c: &mut SourceClock, at_ms: u64) -> HashMap<SpeakerSource, usize> {
        let mut out = HashMap::new();
        c.fill_to(ms(at_ms), |s, x| {
            assert!(x.iter().all(|&v| v == 0.0));
            *out.entry(s).or_insert(0) += x.len();
        });
        out
    }

    #[test]
    fn fill_to_completes_a_silent_source() {
        let mut c = SourceClock::default();
        assert_eq!(feed(&mut c, System, 1_000, 16_000), (0, 16_000));
        let z = fill(&mut c, 2_000);
        assert_eq!(z.get(&System), Some(&16_000));
    }

    #[test]
    fn fill_to_ignores_sources_that_never_delivered() {
        let mut c = SourceClock::default();
        assert!(fill(&mut c, 5_000).is_empty());
    }

    #[test]
    fn fill_to_ignores_on_time_and_jittery_sources() {
        let mut c = SourceClock::default();
        feed(&mut c, Mic, 1_000, 16_000);
        assert!(fill(&mut c, 1_100).is_empty());
        assert!(fill(&mut c, 1_400).is_empty()); // dentro dos 400 ms de tolerância
    }

    #[test]
    fn fill_to_does_not_double_fill() {
        let mut c = SourceClock::default();
        feed(&mut c, System, 1_000, 16_000);
        assert_eq!(fill(&mut c, 2_000).get(&System), Some(&16_000));
        assert!(fill(&mut c, 2_000).is_empty());
        assert!(fill(&mut c, 2_100).is_empty());
        // o próximo pedaço real, em dia com a linha do tempo, também não ganha zeros
        assert_eq!(feed(&mut c, System, 2_100, 1_600), (0, 1_600));
    }

    #[test]
    fn fill_to_is_per_source() {
        let mut c = SourceClock::default();
        feed(&mut c, Mic, 2_000, 32_000); // em dia
        feed(&mut c, System, 1_000, 16_000); // 1 s atrás
        let z = fill(&mut c, 2_000);
        assert_eq!(z.get(&System), Some(&16_000));
        assert_eq!(z.get(&Mic), None);
    }

    #[test]
    fn fill_to_splits_long_gaps_into_one_second_chunks() {
        let mut c = SourceClock::default();
        feed(&mut c, System, 100, 1_600);
        let mut sizes = vec![];
        c.fill_to(ms(2_600), |_, x| sizes.push(x.len()));
        assert_eq!(sizes, vec![16_000, 16_000, 8_000]);
    }
}
