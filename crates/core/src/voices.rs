//! Vozes guardadas: `voices.json` na pasta LOCAL do app (fora do perfil que sincroniza).
//! As impressões de voz existem só neste arquivo e na memória da reunião; a interface
//! recebe `VoiceSummary`, sem impressão. Logs: só contagens e ids, nunca nomes.
//!
//! O arquivo só é gravado quando alguma voz foi nomeada ou alterada (`dirty`): quem nunca
//! nomeou ninguém não ganha um `voices.json`.
use crate::session::write_atomic;
use crate::speaker::{cosine, normalize};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const VOICES_FILE: &str = "voices.json";
const MAX_NAME_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// Modelo que gerou a impressão: impressões de modelos diferentes não se comparam.
    pub model: String,
    pub embedding: Vec<f32>,
    pub created: u64,
    /// Última vez que a voz foi vista (reconhecida ou nomeada numa reunião).
    pub updated: u64,
    /// Falas somadas à impressão.
    pub count: u64,
}

/// O que a janela principal mostra de uma voz guardada: nunca a impressão.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VoiceSummary {
    pub id: String,
    pub name: String,
    pub updated: u64,
    pub count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct VoicesFile {
    v: u32,
    voices: Vec<Voice>,
}

/// Nome aparado e validado; o erro vai para a interface (sem repetir o nome).
pub fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() { bail!("o nome não pode ficar vazio"); }
    if name.chars().count() > MAX_NAME_CHARS { bail!("o nome pode ter no máximo {MAX_NAME_CHARS} caracteres"); }
    Ok(name.to_string())
}

#[derive(Debug)]
pub struct VoiceLibrary {
    path: PathBuf,
    voices: Vec<Voice>,
    /// Há mudanças ainda não gravadas. `AtomicBool` para `save(&self)` poder limpá-lo.
    dirty: AtomicBool,
    /// O arquivo inválido não pôde ser preservado: `save` recusa para não sobrescrevê-lo.
    protect_original: bool,
}

impl VoiceLibrary {
    pub fn empty(path: &Path) -> Self {
        Self { path: path.to_path_buf(), voices: vec![], dirty: AtomicBool::new(false), protect_original: false }
    }

    /// Arquivo ausente: biblioteca vazia. Ilegível ou inválido: biblioteca vazia e o
    /// original renomeado para `voices.json.corrompido-<now>` (devolvido), para que a
    /// próxima gravação não o sobrescreva. Se a cópia não puder ser feita, nada é devolvido
    /// e `save` recusa gravar nesta sessão. O log não leva o conteúdo.
    pub fn load(path: &Path, now: u64) -> (Self, Option<PathBuf>) {
        let parsed = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<VoicesFile>(&bytes).map_err(|e| {
                tracing::warn!(line = e.line(), column = e.column(), "voices.json inválido");
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::empty(path), None),
            Err(e) => {
                tracing::warn!(kind = ?e.kind(), "voices.json ilegível");
                Err(())
            }
        };
        match parsed {
            Ok(f) => {
                let mut lib = Self::empty(path);
                lib.voices = f.voices;
                (lib, None)
            }
            Err(()) => {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(VOICES_FILE);
                let backup = path.with_file_name(format!("{name}.corrompido-{now}"));
                let mut lib = Self::empty(path);
                match fs::rename(path, &backup) {
                    Ok(()) => {
                        tracing::warn!("biblioteca de vozes vazia; original preservado");
                        (lib, Some(backup))
                    }
                    Err(e) => {
                        tracing::error!(kind = ?e.kind(), "não foi possível preservar o voices.json inválido; gravação bloqueada");
                        lib.protect_original = true;
                        (lib, None)
                    }
                }
            }
        }
    }

    /// Só leitura, sem efeito colateral (para a lista da janela principal).
    /// Ausente: lista vazia. Inválido: erro.
    pub fn read_summaries(path: &Path) -> Result<Vec<VoiceSummary>> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e).context("ler as vozes guardadas"),
        };
        let f: VoicesFile = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("o arquivo de vozes guardadas está corrompido"))?;
        let mut lib = Self::empty(path);
        lib.voices = f.voices;
        Ok(lib.summaries())
    }

    /// Grava se houve mudança desde a carga/última gravação; senão não faz nada (e não cria
    /// o arquivo). Recusa se o arquivo inválido original não pôde ser preservado.
    pub fn save(&self) -> Result<()> {
        if !self.dirty.load(Ordering::Relaxed) { return Ok(()); }
        if self.protect_original {
            bail!("o arquivo de vozes guardadas está corrompido e não pôde ser preservado; nada foi gravado");
        }
        if let Some(dir) = self.path.parent() { fs::create_dir_all(dir)?; }
        let file = VoicesFile { v: 1, voices: self.voices.clone() };
        write_atomic(&self.path, &serde_json::to_vec(&file)?)?;
        self.dirty.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn touch(&self) { self.dirty.store(true, Ordering::Relaxed); }

    pub fn len(&self) -> usize { self.voices.len() }
    pub fn is_empty(&self) -> bool { self.voices.is_empty() }
    pub fn get(&self, id: &str) -> Option<&Voice> { self.voices.iter().find(|v| v.id == id) }

    /// Mais recentes primeiro.
    pub fn summaries(&self) -> Vec<VoiceSummary> {
        let mut out: Vec<VoiceSummary> = self.voices.iter()
            .map(|v| VoiceSummary { id: v.id.clone(), name: v.name.clone(), updated: v.updated, count: v.count })
            .collect();
        out.sort_by(|a, b| b.updated.cmp(&a.updated));
        out
    }

    /// Voz guardada do mesmo modelo mais parecida com `emb`, com a semelhança.
    pub fn best_match(&self, model: &str, emb: &[f32]) -> Option<(&Voice, f32)> {
        self.voices.iter()
            .filter(|v| v.model == model)
            .map(|v| (v, cosine(&v.embedding, emb)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Voz do mesmo modelo com esse nome (sem diferenciar maiúsculas).
    pub fn find_name(&self, model: &str, name: &str) -> Option<&Voice> {
        let name = name.trim().to_lowercase();
        self.voices.iter().filter(|v| v.model == model && v.name.to_lowercase() == name)
            .max_by_key(|v| v.updated)
    }

    /// Soma `count` falas (cuja soma das impressões é `sum`) à voz `id`: média ponderada
    /// pelo `count` já guardado.
    pub fn add_to(&mut self, id: &str, sum: &[f32], count: u64, now: u64) -> Result<()> {
        let v = self.voices.iter_mut().find(|v| v.id == id).context("voz guardada inexistente")?;
        if v.embedding.len() != sum.len() { bail!("impressão de voz de tamanho diferente"); }
        let mut merged: Vec<f32> = v.embedding.iter().zip(sum).map(|(e, s)| e * v.count as f32 + s).collect();
        if !normalize(&mut merged) { bail!("impressão de voz inválida"); }
        v.embedding = merged;
        v.count += count;
        v.updated = now;
        self.touch();
        Ok(())
    }

    /// Grava as falas de um grupo com o nome `name`: soma à voz de mesmo nome (e modelo) se
    /// existir, senão cria uma voz nova. Devolve o id.
    pub fn add_named(&mut self, model: &str, name: &str, sum: &[f32], count: u64, now: u64) -> Result<String> {
        let name = clean_name(name)?;
        if let Some(id) = self.find_name(model, &name).map(|v| v.id.clone()) {
            self.add_to(&id, sum, count, now)?;
            return Ok(id);
        }
        let mut embedding = sum.to_vec();
        if !normalize(&mut embedding) { bail!("impressão de voz inválida"); }
        let id = self.new_id(now);
        self.voices.push(Voice { id: id.clone(), name, model: model.into(), embedding, created: now, updated: now, count });
        self.touch();
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        let v = self.voices.iter_mut().find(|v| v.id == id).context("voz guardada inexistente")?;
        v.name = name;
        self.touch();
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let before = self.voices.len();
        self.voices.retain(|v| v.id != id);
        if self.voices.len() == before { bail!("voz guardada inexistente"); }
        self.touch();
        Ok(())
    }

    /// Esvazia a biblioteca e apaga do disco o `voices.json`, o temporário e as cópias
    /// preservadas de arquivos corrompidos (que também contêm impressões de voz). A
    /// biblioteca fica vazia e sem mudanças pendentes: um `save` seguinte não recria o arquivo.
    pub fn delete_all(&mut self) -> Result<()> {
        self.voices.clear();
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or(VOICES_FILE).to_string();
        let prefix = format!("{name}.");
        if let Some(dir) = self.path.parent() {
            let entries = match fs::read_dir(dir) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.dirty.store(false, Ordering::Relaxed);
                    self.protect_original = false;
                    return Ok(());
                }
                Err(e) => return Err(e).context("listar a pasta das vozes guardadas"),
            };
            for entry in entries {
                let entry = entry.context("listar a pasta das vozes guardadas")?;
                let file = entry.file_name().to_string_lossy().into_owned();
                if file == name || file.starts_with(&prefix) {
                    match fs::remove_file(entry.path()) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e).context("apagar o arquivo de vozes"),
                    }
                }
            }
        }
        self.dirty.store(false, Ordering::Relaxed);
        self.protect_original = false;
        Ok(())
    }

    fn new_id(&self, now: u64) -> String {
        (0u32..).map(|n| format!("{now:x}-{n}")).find(|id| self.get(id).is_none()).expect("id livre")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: &str = "modelo-teste";

    fn lib(dir: &Path) -> VoiceLibrary { VoiceLibrary::empty(&dir.join(VOICES_FILE)) }

    #[test]
    fn save_and_load_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        let id = l.add_named(M, "  Ana  ", &[2.0, 0.0], 2, 100).unwrap();
        l.save().unwrap();
        let (back, corrupt) = VoiceLibrary::load(&d.path().join(VOICES_FILE), 200);
        assert!(corrupt.is_none());
        let v = back.get(&id).unwrap();
        assert_eq!((v.name.as_str(), v.count, v.created, v.updated), ("Ana", 2, 100, 100));
        assert_eq!(v.embedding, vec![1.0, 0.0]);
        assert!(std::fs::read_dir(d.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn save_is_a_noop_until_something_changes() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(VOICES_FILE);
        let mut l = lib(d.path());
        l.save().unwrap();
        assert!(!path.exists(), "biblioteca vazia e intacta não cria arquivo");
        let id = l.add_named(M, "Ana", &[1.0, 0.0], 1, 1).unwrap();
        l.save().unwrap();
        assert!(path.exists());
        // Depois de gravar, não está mais suja: apagar o arquivo e salvar de novo não recria.
        std::fs::remove_file(&path).unwrap();
        l.save().unwrap();
        assert!(!path.exists());
        // Cada operação que muda vozes suja de novo.
        l.add_to(&id, &[1.0, 0.0], 1, 2).unwrap();
        l.save().unwrap();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        l.rename(&id, "Ana B").unwrap();
        l.save().unwrap();
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();
        l.delete(&id).unwrap();
        l.save().unwrap();
        assert!(path.exists());
        // Biblioteca carregada e não alterada também não regrava.
        std::fs::remove_file(&path).unwrap();
        let (l2, _) = VoiceLibrary::load(&path, 3);
        l2.save().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn failed_mutations_do_not_mark_dirty() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        assert!(l.rename("nao-existe", "X").is_err());
        assert!(l.delete("nao-existe").is_err());
        assert!(l.add_named(M, "  ", &[1.0], 1, 1).is_err());
        l.save().unwrap();
        assert!(!d.path().join(VOICES_FILE).exists());
    }

    #[test]
    fn weighted_average_by_count() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        // 3 falas em [1, 0]; depois 1 fala em [0, 1] → média ponderada [3, 1] normalizada.
        let id = l.add_named(M, "Ana", &[3.0, 0.0], 3, 1).unwrap();
        l.add_to(&id, &[0.0, 1.0], 1, 2).unwrap();
        let v = l.get(&id).unwrap();
        let n = (10f32).sqrt();
        assert!((v.embedding[0] - 3.0 / n).abs() < 1e-6 && (v.embedding[1] - 1.0 / n).abs() < 1e-6);
        assert_eq!((v.count, v.updated), (4, 2));
    }

    #[test]
    fn same_name_adds_to_existing_voice() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        let a = l.add_named(M, "Ana", &[1.0, 0.0], 1, 1).unwrap();
        let b = l.add_named(M, "ana", &[1.0, 0.0], 1, 2).unwrap();
        assert_eq!(a, b);
        assert_eq!(l.len(), 1);
        assert_eq!(l.get(&a).unwrap().count, 2);
        // Outro modelo: impressões não se comparam, vira outra voz.
        let c = l.add_named("outro-modelo", "Ana", &[1.0, 0.0, 0.0], 1, 3).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn best_match_only_compares_same_model() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        l.add_named("outro-modelo", "Bruno", &[1.0, 0.0], 1, 1).unwrap();
        assert!(l.best_match(M, &[1.0, 0.0]).is_none());
        let a = l.add_named(M, "Ana", &[0.6, 0.8], 1, 1).unwrap();
        let (v, s) = l.best_match(M, &[0.6, 0.8]).unwrap();
        assert_eq!(v.id, a);
        assert!((s - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rename_delete_and_delete_all() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        let a = l.add_named(M, "Ana", &[1.0, 0.0], 1, 1).unwrap();
        let b = l.add_named(M, "Bruno", &[0.0, 1.0], 1, 2).unwrap();
        l.rename(&a, "Ana Paula").unwrap();
        assert_eq!(l.get(&a).unwrap().name, "Ana Paula");
        assert!(l.rename(&a, "   ").is_err());
        assert!(l.rename("nao-existe", "X").is_err());
        assert_eq!(l.summaries().iter().map(|s| s.id.clone()).collect::<Vec<_>>(), vec![b.clone(), a.clone()]);
        l.delete(&a).unwrap();
        assert!(l.get(&a).is_none());
        assert!(l.delete(&a).is_err());
        l.save().unwrap();
        assert!(d.path().join(VOICES_FILE).exists());
        std::fs::write(d.path().join("voices.json.corrompido-5"), "{").unwrap();
        std::fs::write(d.path().join("voices.json.tmp"), "{").unwrap();
        std::fs::write(d.path().join("outro.json"), "{}").unwrap();
        l.delete_all().unwrap();
        assert!(l.is_empty());
        // Salvar depois de apagar tudo não recria o arquivo.
        l.save().unwrap();
        let left: Vec<String> = std::fs::read_dir(d.path()).unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["outro.json".to_string()]);
        assert!(!d.path().join(VOICES_FILE).exists());
        // Nomear de novo suja a biblioteca e o arquivo volta.
        l.add_named(M, "Carla", &[1.0, 0.0], 1, 9).unwrap();
        l.save().unwrap();
        assert!(d.path().join(VOICES_FILE).exists());
    }

    #[test]
    fn corrupt_file_gives_empty_library_and_is_preserved() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(VOICES_FILE);
        std::fs::write(&path, "{ \"v\": 1, \"voices\": [ { quebrado").unwrap();
        assert!(VoiceLibrary::read_summaries(&path).is_err());
        let (mut l, backup) = VoiceLibrary::load(&path, 77);
        assert!(l.is_empty());
        let backup = backup.unwrap();
        assert_eq!(backup, d.path().join("voices.json.corrompido-77"));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ \"v\": 1, \"voices\": [ { quebrado");
        // Sem mudanças, nada é gravado; depois de nomear, grava sem tocar na cópia preservada.
        l.save().unwrap();
        assert!(!path.exists());
        l.add_named(M, "Ana", &[1.0, 0.0], 1, 78).unwrap();
        l.save().unwrap();
        assert!(backup.exists() && path.exists());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ \"v\": 1, \"voices\": [ { quebrado");
    }

    #[test]
    fn corrupt_file_that_cannot_be_preserved_is_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(VOICES_FILE);
        std::fs::write(&path, "{ quebrado").unwrap();
        // Uma pasta no lugar da cópia faz o rename falhar em qualquer plataforma.
        std::fs::create_dir(d.path().join("voices.json.corrompido-77")).unwrap();
        let (mut l, backup) = VoiceLibrary::load(&path, 77);
        assert!(backup.is_none(), "não pode anunciar uma cópia que não existe");
        assert!(l.is_empty());
        l.add_named(M, "Ana", &[1.0, 0.0], 1, 78).unwrap();
        assert!(l.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ quebrado");
        // Apagar tudo remove o original corrompido e libera a gravação.
        std::fs::remove_dir(d.path().join("voices.json.corrompido-77")).unwrap();
        l.delete_all().unwrap();
        assert!(!path.exists());
        l.add_named(M, "Ana", &[1.0, 0.0], 1, 79).unwrap();
        l.save().unwrap();
        assert!(path.exists());
    }

    #[test]
    fn summaries_never_carry_the_embedding() {
        let d = tempfile::tempdir().unwrap();
        let mut l = lib(d.path());
        l.add_named(M, "Ana", &[0.123456, 0.654321], 1, 1).unwrap();
        l.save().unwrap();
        let json = serde_json::to_string(&VoiceLibrary::read_summaries(&d.path().join(VOICES_FILE)).unwrap()).unwrap();
        assert!(!json.contains("embedding"), "{json}");
        assert!(json.contains("\"name\":\"Ana\""));
    }

    #[test]
    fn names_are_trimmed_and_limited() {
        assert_eq!(clean_name("  Ana ").unwrap(), "Ana");
        assert!(clean_name("").is_err());
        assert!(clean_name(&"x".repeat(61)).is_err());
        assert!(clean_name(&"é".repeat(60)).is_ok());
    }

    #[test]
    fn delete_all_reports_failures_instead_of_leaving_voices_behind() {
        // voices.json que não pode ser removido (é uma pasta): erro.
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(VOICES_FILE)).unwrap();
        let mut l = lib(d.path());
        assert!(l.delete_all().is_err());
        // Pasta ilegível (o "pai" é um arquivo): erro, não sucesso silencioso.
        let f = d.path().join("arquivo");
        std::fs::write(&f, "x").unwrap();
        let mut l = VoiceLibrary::empty(&f.join(VOICES_FILE));
        #[cfg(unix)]
        assert!(l.delete_all().is_err());
        // Pasta inexistente: nada a apagar, Ok.
        let mut l = VoiceLibrary::empty(&d.path().join("nao-existe").join(VOICES_FILE));
        assert!(l.delete_all().is_ok());
    }
}
