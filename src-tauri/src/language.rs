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

/// Para onde vai o progresso do preparo de um idioma.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressTo {
    /// Evento `language{preparing}`: o usuário pediu este idioma na reunião em andamento.
    Language,
    /// Evento `models-progress` da janela principal: a reunião acabou e uma reunião nova
    /// pode estar esperando por este download.
    Main,
    /// Pré-carregamento ou pedido cancelado: nada aparece.
    Nowhere,
}

/// O que as janelas devem mostrar agora.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown { Active(Lang), Preparing(Lang) }

#[derive(Debug)]
pub struct LangState {
    active: Lang,
    wanted: Lang,
    ready: HashSet<Lang>,
    loading: HashSet<Lang>,
    /// A reunião acabou: um preparo ainda em andamento não mostra mais nada.
    closed: bool,
    /// Já avisou na tela que a GPU falhou (uma vez por reunião).
    gpu_warned: bool,
}

impl LangState {
    pub fn new(active: Lang) -> Self {
        Self { active, wanted: active, ready: HashSet::from([active]), loading: HashSet::new(),
               closed: false, gpu_warned: false }
    }

    /// Idioma em que o pipeline está transcrevendo.
    pub fn active(&self) -> Lang { self.active }

    /// Último idioma pedido pelo usuário (o app usa `progress_to` e `shown`).
    #[cfg(test)]
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

    /// A reunião acabou: descarta a troca pendente para as janelas.
    pub fn close(&mut self) { self.closed = true; }

    /// Destino do progresso do preparo de `lang`: na reunião, só se o usuário pediu este
    /// idioma; depois que ela acaba, na janela principal (nunca como `language`, que
    /// vazaria para a reunião seguinte).
    pub fn progress_to(&self, lang: Lang) -> ProgressTo {
        if self.closed { ProgressTo::Main }
        else if self.wanted == lang && self.active != lang { ProgressTo::Language }
        else { ProgressTo::Nowhere }
    }

    /// Estado a reenviar às janelas quando um pedido não muda nada (`Step::Nothing`).
    pub fn shown(&self) -> Shown {
        if self.wanted == self.active { Shown::Active(self.active) } else { Shown::Preparing(self.wanted) }
    }

    /// `true` só na primeira vez: o aviso de GPU indisponível aparece uma vez por reunião.
    pub fn warn_gpu_once(&mut self) -> bool { !std::mem::replace(&mut self.gpu_warned, true) }

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
    fn progress_goes_to_language_then_main_window_after_close() {
        let mut s = LangState::new(En);
        assert!(s.preload(Pt));
        assert_eq!(s.progress_to(Pt), ProgressTo::Nowhere); // pré-carregamento é silencioso
        assert_eq!(s.request(Pt), Step::Wait(Pt));
        assert_eq!(s.progress_to(Pt), ProgressTo::Language);
        assert_eq!(s.request(En), Step::Stay(En));
        assert_eq!(s.progress_to(Pt), ProgressTo::Nowhere);
        s.close();
        // Reunião acabou: o download segue e a próxima reunião pode esperar por ele.
        assert_eq!(s.progress_to(Pt), ProgressTo::Main);
    }

    #[test]
    fn repeated_request_resyncs_windows() {
        let mut s = LangState::new(En);
        assert_eq!(s.request(En), Step::Nothing);
        assert_eq!(s.shown(), Shown::Active(En));
        assert_eq!(s.request(Pt), Step::Prepare(Pt));
        assert_eq!(s.request(Pt), Step::Nothing);
        assert_eq!(s.shown(), Shown::Preparing(Pt));
        assert_eq!(s.loaded(Pt), Some(Pt));
        s.switched(Pt);
        assert_eq!(s.request(Pt), Step::Nothing);
        assert_eq!(s.shown(), Shown::Active(Pt));
    }

    #[test]
    fn gpu_warning_once_per_session() {
        let mut s = LangState::new(En);
        assert!(s.warn_gpu_once());
        assert!(!s.warn_gpu_once());
    }

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
