fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word error rate: distância de edição entre palavras / palavras da referência.
pub fn wer(reference: &str, hypothesis: &str) -> f32 {
    let r = words(reference);
    let h = words(hypothesis);
    if r.is_empty() { return if h.is_empty() { 0.0 } else { 1.0 }; }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f32 / r.len() as f32
}

#[cfg(test)]
mod tests {
    use super::wer;
    #[test]
    fn identical_ignoring_case_and_punctuation() {
        assert_eq!(wer("Let's review the timeline.", "let's review the timeline"), 0.0);
    }
    #[test]
    fn one_substitution_in_four_words() {
        assert!((wer("ready by next friday", "ready by last friday") - 0.25).abs() < 1e-6);
    }
    #[test]
    fn empty_hypothesis_is_full_error() {
        assert_eq!(wer("share the budget", ""), 1.0);
    }
}
