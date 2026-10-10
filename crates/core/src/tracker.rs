//! Decide o rótulo de cada fala do áudio do sistema a partir da impressão de voz:
//! 1. voz guardada parecida (≥ `library_threshold`) → nome da voz guardada;
//! 2. senão, grupo da reunião parecido (≥ `group_threshold`) → rótulo do grupo;
//! 3. senão, grupo novo "Participante N".
//! Falas curtas (ou sem impressão) herdam o último falante, sem mexer nos grupos.
use crate::session::{SpeakerEntry, Speakers};
use crate::speaker::{cosine, normalize};
use crate::voices::VoiceLibrary;
use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct TrackerConfig {
    pub library_threshold: f32,
    pub group_threshold: f32,
    pub min_speech_ms: u64,
}

impl Default for TrackerConfig {
    fn default() -> Self { Self { library_threshold: 0.70, group_threshold: 0.65, min_speech_ms: 1_000 } }
}

/// Rótulo de uma fala: grupo da reunião (`"v1"`) e o texto mostrado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub voice: String,
    pub label: String,
}

#[derive(Debug)]
struct Group {
    voice: String,
    label: String,
    /// Voz guardada que recebe as falas do grupo (reconhecida ou nomeada).
    voice_id: Option<String>,
    /// Voz guardada com que o grupo foi reconhecido. Continua trazendo as falas dessa
    /// voz para o grupo mesmo depois de o usuário dar outro nome a ele.
    recognized: Option<String>,
    /// Voz criada ao nomear este grupo NESTA reunião. Só ela pode ser renomeada ou apagada
    /// por um novo nome do grupo; uma voz só reconhecida nunca é alterada.
    created: Option<String>,
    /// Soma das impressões de todas as falas do grupo (o centroide é a soma normalizada).
    sum: Vec<f32>,
    count: u64,
    /// Falas ainda não somadas à voz guardada `voice_id`.
    pending: Vec<f32>,
    pending_count: u64,
}

impl Group {
    fn centroid(&self) -> Vec<f32> {
        let mut c = self.sum.clone();
        normalize(&mut c);
        c
    }

    fn add(&mut self, emb: &[f32]) {
        for (s, e) in self.sum.iter_mut().zip(emb) { *s += e; }
        for (p, e) in self.pending.iter_mut().zip(emb) { *p += e; }
        self.count += 1;
        self.pending_count += 1;
    }

    fn clear_pending(&mut self) {
        self.pending.iter_mut().for_each(|p| *p = 0.0);
        self.pending_count = 0;
    }
}

pub struct SpeakerTracker {
    cfg: TrackerConfig,
    model: String,
    library: VoiceLibrary,
    groups: Vec<Group>,
    last: Option<usize>,
    /// Só falas finais contam; `peek` (parciais) nunca mexe nisto.
    embed_failures: u64,
}

impl SpeakerTracker {
    pub fn new(cfg: TrackerConfig, model: &str, library: VoiceLibrary) -> Self {
        Self { cfg, model: model.into(), library, groups: vec![], last: None, embed_failures: 0 }
    }

    pub fn min_speech_ms(&self) -> u64 { self.cfg.min_speech_ms }

    /// Rótulo de uma fala final do sistema. `emb`: impressão (None se falhou ou não foi
    /// calculada). `None` só quando a fala não tem impressão e ainda não houve falante.
    pub fn assign(&mut self, emb: Option<&[f32]>, speech_ms: u64) -> Option<Assignment> {
        let emb = match emb {
            Some(e) if speech_ms >= self.cfg.min_speech_ms => e,
            _ => return self.last.map(|i| self.assignment(i)),
        };
        let i = self.pick(emb);
        self.groups[i].add(emb);
        self.last = Some(i);
        Some(self.assignment(i))
    }

    /// Rótulo provável de uma fala ainda aberta (parcial), sem criar nem mudar grupos
    /// (`&self`) e sem contar falhas de impressão.
    pub fn peek(&self, emb: &[f32]) -> Option<String> {
        if let Some((v, s)) = self.library.best_match(&self.model, emb) {
            if s >= self.cfg.library_threshold {
                return Some(self.group_for(&v.id).map_or(v.name.clone(), |i| self.groups[i].label.clone()));
            }
        }
        self.best_group(emb).map(|i| self.groups[i].label.clone())
    }

    /// Conta uma falha de impressão de uma fala FINAL (parciais não chamam isto).
    pub fn note_embed_failure(&mut self) { self.embed_failures += 1; }

    #[cfg(test)]
    pub(crate) fn embed_failures(&self) -> u64 { self.embed_failures }

    /// Dá nome ao grupo `voice`: grava a impressão do grupo na biblioteca (voz nova ou
    /// somada à de mesmo nome) e passa a mostrar o nome. Uma voz guardada com que o grupo
    /// tinha sido reconhecido não muda. Renomear de novo um grupo já nomeado nesta reunião
    /// corrige a voz criada (renomeia, ou funde na voz de mesmo nome e apaga a criada),
    /// sem deixar o nome antigo nem contar as falas duas vezes.
    pub fn rename(&mut self, voice: &str, name: &str, now: u64) -> Result<Assignment> {
        let name = crate::voices::clean_name(name)?;
        let i = self.groups.iter().position(|g| g.voice == voice).context("voz inexistente nesta reunião")?;
        let same = self.library.find_name(&self.model, &name).map(|v| v.id.clone());
        let created = self.groups[i].created.clone();

        if let Some(cid) = created {
            match same {
                // Nome livre, ou só difere em maiúsculas da própria voz criada: corrige o nome.
                None => self.library.rename(&cid, &name)?,
                Some(ref s) if *s == cid => self.library.rename(&cid, &name)?,
                // Já existe outra voz com esse nome: funde nela uma vez e apaga a criada.
                Some(other) => {
                    let g = &self.groups[i];
                    self.library.add_to(&other, &g.sum, g.count, now)?;
                    self.library.delete(&cid)?;
                    let g = &mut self.groups[i];
                    g.voice_id = Some(other.clone());
                    g.created = None;
                    g.clear_pending();
                    tracing::info!(voice = %g.voice, id = %other, falas = g.count, "voz nomeada fundida em voz existente");
                }
            }
            self.groups[i].label = name;
            return Ok(self.assignment(i));
        }

        let g = &self.groups[i];
        if same.is_some() && same == g.voice_id {
            // Mesmo nome da voz que o grupo já alimenta: as falas pendentes vão no fim.
            self.groups[i].label = name;
            return Ok(self.assignment(i));
        }
        let id = self.library.add_named(&self.model, &name, &g.sum, g.count, now)?;
        let g = &mut self.groups[i];
        g.label = name;
        g.voice_id = Some(id.clone());
        g.created = if same.is_none() { Some(id.clone()) } else { None };
        g.clear_pending();
        tracing::info!(voice = %g.voice, id = %id, falas = g.count, "voz nomeada");
        Ok(self.assignment(i))
    }

    /// Fim da reunião: soma as falas pendentes às vozes guardadas (média ponderada).
    pub fn finish(&mut self, now: u64) {
        for g in self.groups.iter_mut() {
            let Some(id) = g.voice_id.clone() else { continue };
            if g.pending_count == 0 { continue; }
            if let Err(e) = self.library.add_to(&id, &g.pending, g.pending_count, now) {
                tracing::warn!(id = %id, "voz guardada não atualizada: {e}");
            }
            g.clear_pending();
        }
        let named = self.groups.iter().filter(|g| g.voice_id.is_some()).count();
        tracing::info!(grupos = self.groups.len(), com_voz_guardada = named,
                       falhas_impressao = self.embed_failures, "identificação de vozes encerrada");
    }

    /// Grava a biblioteca se algo mudou (não cria `voices.json` à toa).
    pub fn save_library(&self) -> Result<()> { self.library.save() }

    /// Conteúdo de `speakers.json`.
    pub fn speakers(&self) -> Speakers {
        self.groups.iter()
            .map(|g| (g.voice.clone(), SpeakerEntry { label: g.label.clone(), voice_id: g.voice_id.clone() }))
            .collect()
    }

    fn assignment(&self, i: usize) -> Assignment {
        Assignment { voice: self.groups[i].voice.clone(), label: self.groups[i].label.clone() }
    }

    fn group_for(&self, id: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.recognized.as_deref() == Some(id) || g.voice_id.as_deref() == Some(id))
    }

    fn best_group(&self, emb: &[f32]) -> Option<usize> {
        self.groups.iter().enumerate()
            .map(|(i, g)| (i, cosine(&g.centroid(), emb)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, s)| *s >= self.cfg.group_threshold)
            .map(|(i, _)| i)
    }

    fn pick(&mut self, emb: &[f32]) -> usize {
        let hit = self.library.best_match(&self.model, emb)
            .filter(|(_, s)| *s >= self.cfg.library_threshold)
            .map(|(v, _)| (v.id.clone(), v.name.clone()));
        if let Some((id, name)) = hit {
            if let Some(i) = self.group_for(&id) { return i; }
            let i = self.create(emb.len(), name, Some(id.clone()));
            tracing::info!(voice = %self.groups[i].voice, id = %id, "voz reconhecida");
            return i;
        }
        if let Some(i) = self.best_group(emb) { return i; }
        let label = format!("Participante {}", self.groups.len() + 1);
        self.create(emb.len(), label, None)
    }

    fn create(&mut self, dim: usize, label: String, recognized: Option<String>) -> usize {
        let voice = format!("v{}", self.groups.len() + 1);
        self.groups.push(Group {
            voice, label, voice_id: recognized.clone(), recognized, created: None,
            sum: vec![0.0; dim], count: 0, pending: vec![0.0; dim], pending_count: 0,
        });
        self.groups.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voices::VOICES_FILE;

    const M: &str = "modelo-teste";
    /// Impressões falsas em 3 dimensões: A e B quase ortogonais; A2 parecida com A.
    const A: [f32; 3] = [1.0, 0.0, 0.0];
    const A2: [f32; 3] = [0.96, 0.28, 0.0];
    const B: [f32; 3] = [0.0, 1.0, 0.0];
    const C: [f32; 3] = [0.0, 0.0, 1.0];

    fn tracker(dir: &std::path::Path) -> SpeakerTracker {
        SpeakerTracker::new(TrackerConfig::default(), M, VoiceLibrary::empty(&dir.join(VOICES_FILE)))
    }

    fn a(voice: &str, label: &str) -> Option<Assignment> {
        Some(Assignment { voice: voice.into(), label: label.into() })
    }

    #[test]
    fn creates_groups_below_threshold_and_joins_similar_ones() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        assert_eq!(t.assign(Some(&A), 2_000), a("v1", "Participante 1"));
        assert_eq!(t.assign(Some(&B), 2_000), a("v2", "Participante 2"));
        assert_eq!(t.assign(Some(&A2), 2_000), a("v1", "Participante 1"));
        assert_eq!(t.assign(Some(&C), 2_000), a("v3", "Participante 3"));
    }

    #[test]
    fn joining_updates_the_centroid() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        // D está a 0,6 de A (abaixo do limite 0,65), mas a 0,82 do centroide de A + a2.
        let a2 = [0.8, 0.6, 0.0];
        let d_ = [0.6, 0.8, 0.0];
        assert_eq!(t.assign(Some(&a2), 2_000), a("v1", "Participante 1"));
        assert!(cosine(&A, &d_) < 0.65);
        assert_eq!(t.assign(Some(&d_), 2_000), a("v1", "Participante 1"));
    }

    #[test]
    fn short_or_failed_speech_inherits_last_speaker_without_touching_groups() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        assert_eq!(t.assign(Some(&A), 500), None); // curta e sem falante anterior
        assert_eq!(t.assign(None, 3_000), None);
        t.assign(Some(&A), 2_000);
        t.assign(Some(&B), 2_000);
        // Curta com a impressão de A: herda B (o último), sem criar grupo nem mexer em A.
        assert_eq!(t.assign(Some(&A), 900), a("v2", "Participante 2"));
        t.note_embed_failure();
        assert_eq!(t.assign(None, 5_000), a("v2", "Participante 2"));
        assert_eq!(t.speakers().len(), 2);
        assert_eq!(t.groups[1].count, 1);
    }

    #[test]
    fn recognizes_saved_voice_above_threshold() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = VoiceLibrary::empty(&d.path().join(VOICES_FILE));
        let ana = lib.add_named(M, "Ana", &A, 5, 1).unwrap();
        let mut t = SpeakerTracker::new(TrackerConfig::default(), M, lib);
        assert_eq!(t.assign(Some(&A2), 2_000), a("v1", "Ana"));
        assert_eq!(t.assign(Some(&A), 2_000), a("v1", "Ana"));
        assert_eq!(t.assign(Some(&B), 2_000), a("v2", "Participante 2"));
        assert_eq!(t.speakers()["v1"].voice_id.as_deref(), Some(ana.as_str()));
        assert_eq!(t.peek(&A2).as_deref(), Some("Ana"));
        assert_eq!(t.peek(&C), None);
    }

    #[test]
    fn peek_changes_nothing_and_counts_no_failures() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        let before = (t.groups.len(), t.groups[0].count, t.last, t.embed_failures);
        assert_eq!(t.peek(&A2).as_deref(), Some("Participante 1"));
        assert_eq!(t.peek(&C), None);
        assert_eq!((t.groups.len(), t.groups[0].count, t.last, t.embed_failures), before);
        assert_eq!(t.embed_failures, 0);
    }

    #[test]
    fn renaming_creates_saved_voice_and_relabels_group() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        t.assign(Some(&A2), 2_000);
        assert_eq!(Some(t.rename("v1", "  Bruno ", 10).unwrap()), a("v1", "Bruno"));
        let s = t.speakers();
        let id = s["v1"].voice_id.clone().unwrap();
        let v = t.library.get(&id).unwrap();
        assert_eq!((v.name.as_str(), v.count), ("Bruno", 2));
        assert_eq!(t.assign(Some(&A), 2_000), a("v1", "Bruno"));
        assert!(t.rename("v9", "X", 11).is_err());
        assert!(t.rename("v1", "  ", 11).is_err());
    }

    #[test]
    fn renaming_recognized_group_does_not_change_original_voice() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = VoiceLibrary::empty(&d.path().join(VOICES_FILE));
        let ana = lib.add_named(M, "Ana", &A, 5, 1).unwrap();
        let before = lib.get(&ana).unwrap().clone();
        let mut t = SpeakerTracker::new(TrackerConfig::default(), M, lib);
        t.assign(Some(&A2), 2_000);
        t.rename("v1", "Bruno", 10).unwrap();
        // Novas falas da mesma pessoa ainda casam com "Ana" na biblioteca, mas vêm para o grupo.
        assert_eq!(t.assign(Some(&A), 2_000), a("v1", "Bruno"));
        t.finish(20);
        assert_eq!(t.library.get(&ana).unwrap(), &before);
        let bruno = t.speakers()["v1"].voice_id.clone().unwrap();
        assert_ne!(bruno, ana);
        assert_eq!(t.library.get(&bruno).unwrap().count, 2); // 1 no nome + 1 pendente no fim
    }

    #[test]
    fn finish_adds_pending_speech_by_weighted_average() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = VoiceLibrary::empty(&d.path().join(VOICES_FILE));
        let ana = lib.add_named(M, "Ana", &[3.0, 0.0, 0.0], 3, 1).unwrap();
        let mut t = SpeakerTracker::new(TrackerConfig { library_threshold: 0.5, ..Default::default() }, M, lib);
        let x = [0.8, 0.6, 0.0];
        assert_eq!(t.assign(Some(&x), 2_000), a("v1", "Ana"));
        // Confirmar o mesmo nome não soma de novo: as falas entram uma vez, no fim.
        t.rename("v1", "Ana", 5).unwrap();
        assert_eq!(t.library.get(&ana).unwrap().count, 3);
        t.finish(9);
        let v = t.library.get(&ana).unwrap();
        assert_eq!((v.count, v.updated), (4, 9));
        let mut want = vec![3.0 + 0.8, 0.6, 0.0];
        normalize(&mut want);
        for (g, w) in v.embedding.iter().zip(&want) { assert!((g - w).abs() < 1e-6); }
        t.save_library().unwrap();
        assert!(d.path().join(VOICES_FILE).exists());
    }

    #[test]
    fn meeting_without_naming_or_recognition_writes_no_voices_file() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        t.assign(Some(&B), 2_000);
        t.finish(5);
        t.save_library().unwrap();
        assert!(!d.path().join(VOICES_FILE).exists());
    }

    #[test]
    fn fixing_a_typo_renames_the_session_voice_instead_of_adding_another() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        t.assign(Some(&A2), 2_000);
        t.rename("v1", "Brnuo", 10).unwrap();
        t.rename("v1", "Bruno", 11).unwrap();
        assert_eq!(t.library.len(), 1);
        let id = t.speakers()["v1"].voice_id.clone().unwrap();
        let v = t.library.get(&id).unwrap();
        assert_eq!((v.name.as_str(), v.count), ("Bruno", 2));
        assert!(t.library.find_name(M, "Brnuo").is_none());
        assert_eq!(t.assign(Some(&A), 2_000), a("v1", "Bruno"));
    }

    #[test]
    fn renaming_back_and_forth_does_not_double_count() {
        let d = tempfile::tempdir().unwrap();
        let mut t = tracker(d.path());
        t.assign(Some(&A), 2_000);
        t.assign(Some(&A2), 2_000);
        t.rename("v1", "Bruno", 10).unwrap();
        t.rename("v1", "Carlos", 11).unwrap();
        t.rename("v1", "Bruno", 12).unwrap();
        assert_eq!(t.library.len(), 1);
        let id = t.speakers()["v1"].voice_id.clone().unwrap();
        assert_eq!(t.library.get(&id).unwrap().count, 2);
        t.finish(20);
        assert_eq!(t.library.get(&id).unwrap().count, 2);
    }

    #[test]
    fn renaming_to_an_existing_voice_merges_once_and_drops_the_session_voice() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = VoiceLibrary::empty(&d.path().join(VOICES_FILE));
        let ana = lib.add_named(M, "Ana", &C, 5, 1).unwrap();
        let mut t = SpeakerTracker::new(TrackerConfig::default(), M, lib);
        t.assign(Some(&A), 2_000);
        t.assign(Some(&A2), 2_000);
        t.rename("v1", "Zed", 10).unwrap();
        assert_eq!(t.library.len(), 2);
        assert_eq!(Some(t.rename("v1", "ana", 11).unwrap()), a("v1", "ana"));
        assert_eq!(t.library.len(), 1);
        assert!(t.library.find_name(M, "Zed").is_none());
        assert_eq!(t.speakers()["v1"].voice_id.as_deref(), Some(ana.as_str()));
        assert_eq!(t.library.get(&ana).unwrap().count, 7);
        t.finish(20);
        assert_eq!(t.library.get(&ana).unwrap().count, 7);
    }

    #[test]
    fn renaming_a_recognized_group_twice_never_touches_the_original() {
        let d = tempfile::tempdir().unwrap();
        let mut lib = VoiceLibrary::empty(&d.path().join(VOICES_FILE));
        let ana = lib.add_named(M, "Ana", &A, 5, 1).unwrap();
        let before = lib.get(&ana).unwrap().clone();
        let mut t = SpeakerTracker::new(TrackerConfig::default(), M, lib);
        t.assign(Some(&A2), 2_000);
        t.rename("v1", "Brnuo", 10).unwrap();
        t.rename("v1", "Bruno", 11).unwrap();
        assert_eq!(t.library.get(&ana).unwrap(), &before);
        assert_eq!(t.library.len(), 2);
        assert_eq!(t.library.find_name(M, "Bruno").map(|v| v.count), Some(1));
    }
}
