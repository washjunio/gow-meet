//! Idioma de uma reunião em andamento (etapa 3): o que vale no pipeline, o último pedido
//! do usuário e os pares de modelos já entregues ao pipeline ou em preparo.
use gow_meet_core::lang::Lang;
use std::collections::HashSet;

/// O que fazer com um pedido de troca de idioma.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Já é o idioma pedido, ou ele já está a caminho.
    Nothing,
    /// Volta ao idioma atual, cancelando uma preparação: só avisar as janelas.
    Stay(Lang),
    /// Par já entregue ao pipeline: trocar agora.
    Switch(Lang),
    /// Par em preparo (ex.: pré-carregamento): trocar quando ficar pronto.
    Wait(Lang),
    /// Par ausente: começar a preparação (baixar se preciso e carregar).
    Prepare(Lang),
}

#[derive(Debug)]
pub struct LangState { active: Lang, wanted: Lang, ready: HashSet<Lang>, loading: HashSet<Lang> }

impl LangState {
    pub fn new(active: Lang) -> Self {
        Self { active, wanted: active, ready: HashSet::from([active]), loading: HashSet::new() }
    }

    /// Idioma em que o pipeline está transcrevendo.
    pub fn active(&self) -> Lang { self.active }

    /// Último idioma pedido pelo usuário.
    pub fn wanted(&self) -> Lang { self.wanted }

    /// Pré-carregamento ao iniciar, sem pedido do usuário. `false` se já está pronto ou em preparo.
    pub fn preload(&mut self, lang: Lang) -> bool {
        !self.ready.contains(&lang) && self.loading.insert(lang)
    }

    /// Vale o último pedido: um pedido novo substitui o anterior.
    pub fn request(&mut self, lang: Lang) -> Step {
        if lang == self.wanted { return Step::Nothing; }
        self.wanted = lang;
        if lang == self.active { return Step::Stay(lang); }
        if self.ready.contains(&lang) { return Step::Switch(lang); }
        if self.loading.contains(&lang) { return Step::Wait(lang); }
        self.loading.insert(lang);
        Step::Prepare(lang)
    }

    /// Par entregue ao pipeline. `Some(lang)` se ele é o pedido atual: trocar agora.
    pub fn loaded(&mut self, lang: Lang) -> Option<Lang> {
        self.loading.remove(&lang);
        self.ready.insert(lang);
        (self.wanted == lang && self.active != lang).then_some(lang)
    }

    pub fn switched(&mut self, lang: Lang) { self.active = lang; }

    /// Preparação falhou. `true` se era o pedido atual: o pedido volta ao idioma atual e as
    /// janelas precisam ser avisadas. Pré-carregamento que falha não avisa ninguém.
    pub fn failed(&mut self, lang: Lang) -> bool {
        self.loading.remove(&lang);
        if self.wanted != lang { return false; }
        self.wanted = self.active;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Lang::*;

    #[test]
    fn current_language_needs_nothing() {
        let mut s = LangState::new(En);
        assert_eq!(s.request(En), Step::Nothing);
        assert_eq!((s.active(), s.wanted()), (En, En));
    }

    #[test]
    fn absent_pair_is_prepared_then_switched_when_loaded() {
        let mut s = LangState::new(En);
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
        assert_eq!(s.request(Pt), Step::Nothing); // já a caminho
        assert_eq!(s.loaded(Pt), Some(Pt));
        s.switched(Pt);
        assert_eq!(s.active(), Pt);
        assert_eq!(s.request(En), Step::Switch(En)); // o inglês continua carregado
    }

    #[test]
    fn asking_back_cancels_pending_switch() {
        let mut s = LangState::new(En);
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
        assert_eq!(s.request(En), Step::Stay(En));
        assert_eq!(s.loaded(Pt), None);
        assert_eq!(s.active(), En);
        assert_eq!(s.request(Pt), Step::Switch(Pt));
    }

    #[test]
    fn request_during_preload_waits() {
        let mut s = LangState::new(En);
        assert!(s.preload(Pt));
        assert!(!s.preload(Pt));
        assert_eq!(s.request(Pt), Step::Wait(Pt));
        assert_eq!(s.loaded(Pt), Some(Pt));
    }

    #[test]
    fn failed_request_returns_to_active_and_can_retry() {
        let mut s = LangState::new(En);
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
        assert!(s.failed(Pt));
        assert_eq!(s.wanted(), En);
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
    }

    #[test]
    fn failed_preload_is_silent() {
        let mut s = LangState::new(En);
        assert!(s.preload(Pt));
        assert!(!s.failed(Pt));
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
    }
}
