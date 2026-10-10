# gow-meet Etapa 3 — Reuniões em português e troca de idioma — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Usar o app também em reuniões em português (tradução para o inglês), escolhendo o idioma falado antes de iniciar e trocando de idioma durante a reunião com um clique.

**Architecture:** `gow-meet-core` ganha o tipo `Lang { En, Pt }` (`lang.rs`), o campo `lang` em cada `Line`, filtro de alucinações por idioma e separador de idioma no Markdown. No pipeline, cada fala é **carimbada com o idioma** que valia quando foi enviada à fila (o carimbo é o "marcador" da spec); as threads de transcrição e tradução guardam um modelo por idioma, e os modelos do segundo idioma chegam pela própria fila (`add_language`) antes de qualquer fala desse idioma. `set_language` fecha as falas abertas no idioma antigo e passa a carimbar o novo. O app Tauri garante e carrega o par do idioma escolhido ao iniciar, pré-carrega o outro em segundo plano se ele já está no disco, e expõe `set_language` com um pequeno estado (`LangState`) que decide entre trocar já, esperar, preparar (baixar e carregar) ou cancelar. A interface ganha o seletor na janela principal, o botão "EN → PT" na flutuante e o separador "— agora em português —".

**Tech Stack:** Rust stable, whisper.cpp (`whisper-rs`), CTranslate2 (`ct2rs`), Tauri v2, React 18 + TypeScript + Vitest. Modelos novos: whisper.cpp `ggml-small-q5_1.bin` (multilíngue, MIT) e um Opus-MT pt→en (Apache-2.0) convertido para CTranslate2 e publicado em `gow-solucoes/opus-mt-pt-en-ct2`.

**Spec:** `docs/superpowers/specs/2026-10-10-gow-meet-etapa3-idiomas-design.md`

## Global Constraints

- Processamento 100% local. Nada de áudio, texto ou impressão de voz sai da máquina.
- O áudio de reunião nunca é gravado em disco. Fixtures de teste são frases fictícias geradas com `say`.
- Logs nunca contêm texto de reunião nem nomes de pessoas. Para idioma: só código e status (`idioma trocado de=en para=pt`, `par de idioma pronto lang=pt ms=2100`).
- Modelos com licença que permita uso em empresa (`cc-by-nc-*` eliminatório; NLLB proibido), baixados com revisão fixa e conferidos por SHA-256 (manifesto `crates/core/models.json`).
- Idiomas só EN e PT. A tradução é sempre para o outro idioma. Códigos no JSON e no Whisper: `"en"`, `"pt"`.
- Inglês continua com `whisper-small-en` + `opus-mt-en-pt` com prefixo `>>pob<<` (sem piora).
- Ids do manifesto novos: `whisper-small` (Whisper multilíngue para o PT) e `opus-mt-pt-en` (tradução pt→en). Se a medição da Task 1 reprovar o `small`, o id do Whisper PT passa a ser `whisper-medium` (Task 1 registra a escolha em `docs/modelos-pt.md`).
- `Line` ganha `lang` (`"en"`/`"pt"`, sempre gravado); linha sem o campo (sessões antigas) é inglês.
- Contrato com o front-end: `ensure_models {lang}`, `start_session {title, lang}`, `set_language {lang}` (novo); evento `"gow-meet"` com `kind: "language"`, `{lang, status: "preparing" | "active" | "failed", progress?}` (`progress` de 0 a 1, só enquanto baixa); `partial` ganha `lang`; `Line.lang` no TypeScript.
- `set_language` declarado no `AppManifest` de `src-tauri/build.rs` e permitido em `capabilities/main.json` **e** `capabilities/overlay.json`.
- `config.json`: chaves atuais (`whisper_model`, `opus_model`, `opus_prefix`) valem para o EN; novas `whisper_model_pt`, `opus_model_pt`, `opus_prefix_pt`. `source_lang`/`target_lang` saem do `Config` (um arquivo antigo com elas continua carregando).
- Garantias das etapas 1 e 2 continuam: eventos de um mesmo `id` em ordem (lock de `OpenPartials`), pânico de modelo isolado com aviso genérico único, rótulos de voz, ordem de locks do pipeline (rastreador → store → parciais abertas). No app, ordem de locks: slot → estado do idioma.
- Visual GOW (`src/styles.css`): Rajdhani caixa alta nos rótulos; amarelo só no idioma ativo e em destaques.
- Testes e exemplos só com frases e nomes fictícios.
- Compilar com `source "$HOME/.cargo/env"; export CMAKE_POLICY_VERSION_MINIMUM=3.5`.
- Toda mensagem de commit termina com uma linha em branco e exatamente:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2
  ```
  (nos passos de commit isso é feito com um segundo `-m`).
- Publicar no Hugging Face e fazer `git push` só com OK explícito do usuário.
- Fora do escopo: detecção automática de idioma, outros idiomas, idioma de tradução separado, idiomas diferentes por fonte, retraduzir falas antigas.

## Ajustes em relação à spec (para revisão)

1. **O "marcador de idioma" é um carimbo por fala.** Em vez de um item especial na fila, cada job de transcrição leva o idioma que valia quando foi enviado (`AsrJob.lang`), e a thread de tradução usa o idioma da própria fala (`Line.lang`). O efeito é o da spec — o que veio antes da troca sai no idioma antigo, o que veio depois no novo, e nenhuma fala mistura idiomas — com menos estado. `coalesce` não precisa mudar: `set_language` fecha as falas abertas antes de trocar, então nenhuma fala (mesmo `seq`) tem jobs de dois idiomas. Os modelos do segundo idioma, esses sim, vão pela fila (`AsrMsg::Load`), antes de qualquer fala carimbada com ele.
2. **`Line.lang` não é opcional:** padrão `en` ao ler linhas antigas. Toda sessão antiga é em inglês, então é o mesmo que "vale o `source_lang` do `meta.json`".
3. **Separador na janela flutuante** vem da comparação do idioma entre falas vizinhas, sem evento próprio. "Histórico" no app é o `transcript.md` aberto pela lista de reuniões: o separador vai no Markdown.
4. **Falha ao preparar o outro idioma:** o backend emite `language {failed}` seguido de `language {active}` do idioma atual; a interface monta o aviso ("Não foi possível preparar o português; a reunião continua em inglês."). Como os demais avisos, ele some na próxima fala final (minor M2 da etapa 2, já registrado).
5. **Downloads serializados:** `ensure_pair` roda sob um lock próprio do app (`ensuring`), para que um download do outro idioma ainda em curso (a reunião acabou, mas o download continua) e o `ensure_models` da reunião seguinte não escrevam no mesmo `.part`.
6. **Aviso de GPU uma vez:** só o par inicial avisa na tela; o segundo par cai para o processador com registro no log.
7. **Progresso por arquivo:** `progress` é a fração do arquivo sendo baixado (o Whisper domina o tamanho).
8. **Pré-carregamento confere o hash** (uma vez por execução do app, como hoje) em segundo plano; custa alguns segundos de CPU no início da reunião.
9. **Pânico num idioma desliga só aquele idioma** (o modelo sai do mapa); o outro continua funcionando.

## Review Focus

1. Clicar para trocar no meio de uma fala longa → a fala fecha no idioma antigo e o resto sai no novo, sem fala com transcrição de um idioma e tradução do outro (Task 3: `switch_closes_open_utterances_in_old_language`).
2. Pedir português, e voltar para inglês antes do português ficar pronto → quando o português termina de carregar, nada troca; pedir português de novo troca na hora (Task 4: `asking_back_cancels_pending_switch`).
3. Encerrar a reunião enquanto o outro idioma prepara → o encerramento não pode falhar com "pipeline ainda em uso" nem travar; os modelos carregados são descartados (Task 4: a thread de preparo nunca guarda o `Arc<Pipeline>`, só o id da sessão; teste `prepare_for_ended_session_is_ignored` cobre o estado).
4. Iniciar uma reunião nova enquanto o download do outro idioma da reunião anterior ainda corre → os dois não podem baixar o mesmo arquivo ao mesmo tempo (Task 4: lock `ensuring`; teste `ensure_is_serialized`).
5. Pânico no Whisper português → o inglês continua funcionando depois de voltar (Task 3: `panic_in_one_language_keeps_the_other`).

## Estrutura de arquivos

```text
crates/core/
  models.json                     + "whisper-small", "opus-mt-pt-en"
  src/lib.rs                      + pub mod lang
  src/lang.rs                     NOVO: Lang { En, Pt }, code, other, from_code, name
  src/asr.rs                      clean_transcript(raw, lang): alucinações por idioma
  src/session.rs                  Line.lang; SessionWriter::add_models; Markdown com separador
  src/pipeline.rs                 PipelineConfig.lang, AsrJob.lang, AsrMsg, add_language, set_language,
                                  language, add_models; modelos por idioma nas duas threads; Partial.lang
  src/models.rs                   teste do manifesto com os ids novos
  tests/asr_whisper.rs            clean_transcript com Lang::En
  tests/asr_whisper_pt.rs         NOVO (ignorado): WER e silêncio em português
  tests/translate_ct2_pt.rs       NOVO (ignorado): frases de reunião pt→en
  tests/lang_switch.rs            NOVO (ignorado): troca de idioma com os modelos reais
  tests/fixtures/pt/              NOVO: s1..s5 (.wav/.txt) e meeting.wav, voz Luciana do `say`
crates/cli/src/main.rs            --lang en|pt
scripts/gen-fixtures-pt.sh        NOVO
scripts/convert-opus-mt.sh        aceita o modelo de origem como 2º argumento
docs/modelos-pt.md                NOVO: medição e escolha (só números)
docs/validacao-idiomas.md         NOVO (Task 6)
README.md                         licenças, config.json, CLI --lang
src-tauri/
  build.rs                        + set_language
  capabilities/main.json          + allow-set-language
  capabilities/overlay.json       + allow-set-language
  permissions/autogenerated/set_language.toml   gerado pelo build (commitado)
  src/config.rs                   chaves PT, Pair, Config::pair; sem source_lang/target_lang
  src/language.rs                 NOVO: LangState, Step
  src/lib.rs                      UiEvent::Language, LangStatus, Partial.lang; pares por idioma;
                                  ensure_pair, load_pair, spawn_prepare; set_language; start com idioma
src/
  lib/lang.ts                     NOVO: Lang, LANGS, LANG_NAME, other, pairLabel, switchedTo,
                                  toggleTarget, loadLang, saveLang
  lib/lang.test.ts                NOVO
  lib/transcript.ts               Line.lang, Entry.lang, State.language, evento language, langBreak
  lib/transcript.test.ts          + idioma
  lib/status.ts                   renameError → actionError
  lib/status.test.ts              idem
  Overlay.tsx                     botão de idioma, separador, onLang
  Overlay.test.tsx                + idioma
  MainWindow.tsx                  seletor de idioma; ensure/start com idioma; set_language
  demo/Demo.tsx                   estados main-idle-pt, main-running-preparing, overlay-languages, overlay-preparing
  styles.css                      .lang-picker, .lang-option, .lang-toggle, .lang-break
```

## Evidências levantadas antes do plano

- `ggerganov/whisper.cpp` na revisão já usada (`5359861c739e955e79d9a303bcbc70fb988958b1`) tem `ggml-small-q5_1.bin` (multilíngue), 190 085 487 bytes, SHA-256 `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`; e `ggml-medium-q5_0.bin`, 539 212 467 bytes, SHA-256 `19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f` (alternativa).
- Não existe `opus-mt-tc-big-pt-en`. Candidatos pt→en da Helsinki-NLP, todos Apache-2.0:
  - `Helsinki-NLP/opus-mt-itc-en` (revisão `ce67164f0490e7b701825524982d4cc8f5188212`), Marian base, origem multilíngue, sem prefixo;
  - `Helsinki-NLP/opus-mt-ROMANCE-en` (revisão `e9ca9975e3972afd80732f08ce01d3a1339f47f8`), Marian base, sem prefixo;
  - `Helsinki-NLP/opus-mt-tc-bible-big-itc-deu_eng_fra_por_spa` (revisão `4bcdcc2580bef62cae46ec574655c22219f3aaf3`), "big", destino multilíngue: prefixo `>>eng<<`.
- O `say` deste Mac tem a voz `Luciana` (pt_BR).
- Vitest roda em ambiente `node` (sem DOM): testes de componente usam `renderToStaticMarkup`; lógica de clique fica em funções puras testáveis.
- O segmentador emite parcial a cada 1 000 ms e fecha a fala com 600 ms de silêncio; fala mínima 300 ms.

---

### Task 1: Medição dos modelos em português, escolha do pt→en e manifesto

**Files:**
- Create: `scripts/gen-fixtures-pt.sh`, `crates/core/tests/fixtures/pt/*` (gerados), `crates/core/tests/asr_whisper_pt.rs`, `crates/core/tests/translate_ct2_pt.rs`, `docs/modelos-pt.md`
- Modify: `scripts/convert-opus-mt.sh`, `crates/cli/src/main.rs`, `crates/core/models.json`, `crates/core/src/models.rs`, `README.md`

**Interfaces:**
- Consumes: `WhisperTranscriber::new(path, lang_code, use_gpu)`, `Ct2Translator::new(dir, prefix)`, `clean_transcript(raw)` (ainda com um argumento; a Task 2 acrescenta o idioma), `wer::wer`.
- Produces: ids `whisper-small` (ou `whisper-medium`) e `opus-mt-pt-en` no manifesto; em `docs/modelos-pt.md`, as seções **Escolha** (id do Whisper PT, candidato pt→en, prefixo pt→en: nenhum ou `>>eng<<`) e **Alucinações** (frases que o Whisper inventou em silêncio em português, em minúsculas). As Tasks 2 e 4 usam esses valores.

- [ ] **Step 1: Script das frases fictícias em português**

`scripts/gen-fixtures-pt.sh`:
```bash
#!/usr/bin/env bash
# Gera áudio fictício em português com a voz Luciana do macOS. Só frases inventadas.
set -euo pipefail
OUT=crates/core/tests/fixtures/pt
mkdir -p "$OUT"
say_wav() { say -v Luciana --data-format=LEI16@16000 -o "$OUT/$1.wav" "$2"; printf '%s\n' "$2" > "$OUT/$1.txt"; }
say_wav s1 "Vamos revisar o cronograma do projeto na próxima semana."
say_wav s2 "O novo painel precisa ficar pronto até sexta-feira."
say_wav s3 "Você pode compartilhar a estimativa de orçamento com a equipe?"
say_wav s4 "Precisamos decidir quais funcionalidades entram na primeira versão."
say_wav s5 "Vou mandar um resumo depois desta reunião."
say -v Luciana --data-format=LEI16@16000 -o "$OUT/meeting.wav" \
  "Bom dia a todos. [[slnc 1500]] Vamos revisar o cronograma do projeto na próxima semana. [[slnc 1500]] \
   O novo painel precisa ficar pronto até sexta-feira. [[slnc 1500]] Você pode compartilhar a estimativa de orçamento com a equipe? \
   [[slnc 1500]] Vou mandar um resumo depois desta reunião."
ls -la "$OUT"
```
Run: `chmod +x scripts/gen-fixtures-pt.sh && scripts/gen-fixtures-pt.sh`
Expected: 11 arquivos em `crates/core/tests/fixtures/pt/` (5 `.wav` + 5 `.txt` + `meeting.wav`), cada `.wav` com dezenas de KB.

- [ ] **Step 2: `--lang` no CLI**

Em `crates/cli/src/main.rs`, acrescente ao `Args` (depois de `prefix`):
```rust
    /// Idioma falado no áudio: en (padrão) ou pt.
    #[arg(long, default_value = "en")] lang: String,
```
e troque, em `main`:
```rust
    let asr = WhisperTranscriber::new(&a.whisper, &a.lang, !a.cpu)?;
```
e no `Meta`:
```rust
        ended: None, source_lang: a.lang.clone(),
        target_lang: if a.lang == "pt" { "en".into() } else { "pt".into() }, models: Default::default(),
```
Run: `cargo build -p gow-meet-cli --release`
Expected: compila sem avisos novos.

- [ ] **Step 3: Testes ignorados de transcrição em português**

`crates/core/tests/asr_whisper_pt.rs`:
```rust
//! Rode com: GOW_MEET_WHISPER_PT=$PWD/models/tmp/ggml-small-q5_1.bin cargo test -p gow-meet-core --release --test asr_whisper_pt -- --ignored --nocapture
use gow_meet_core::asr::{clean_transcript, Transcriber, WhisperTranscriber};
use gow_meet_core::{wav::read_wav_16k_mono, wer::wer};
use std::path::{Path, PathBuf};

fn model() -> PathBuf { PathBuf::from(std::env::var("GOW_MEET_WHISPER_PT").expect("defina GOW_MEET_WHISPER_PT")) }

#[test]
#[ignore]
fn portuguese_sentences_have_low_wer() {
    let mut t = WhisperTranscriber::new(&model(), "pt", true).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pt");
    let mut total = 0.0;
    for i in 1..=5 {
        let audio = read_wav_16k_mono(&dir.join(format!("s{i}.wav"))).unwrap();
        let reference = std::fs::read_to_string(dir.join(format!("s{i}.txt"))).unwrap();
        let start = std::time::Instant::now();
        let hyp = clean_transcript(&t.transcribe(&audio).unwrap()).unwrap_or_default();
        let e = wer(&reference, &hyp);
        eprintln!("s{i}: wer={e:.2} ({:?}) | {hyp}", start.elapsed());
        total += e;
    }
    assert!(total / 5.0 < 0.20, "WER médio = {}", total / 5.0);
}

/// Imprime o que o Whisper devolve em silêncio e ruído baixo: alimenta a lista de alucinações PT.
#[test]
#[ignore]
fn silence_in_portuguese() {
    let mut t = WhisperTranscriber::new(&model(), "pt", true).unwrap();
    let noise: Vec<f32> = (0..16_000 * 3).map(|i| 0.002 * ((i * 7919 % 1000) as f32 / 1000.0 - 0.5)).collect();
    for (name, audio) in [("silêncio", vec![0.0; 16_000 * 3]), ("ruído", noise)] {
        let raw = t.transcribe(&audio).unwrap();
        eprintln!("{name}: bruto={raw:?} limpo={:?}", clean_transcript(&raw));
    }
}
```

- [ ] **Step 4: Teste ignorado de tradução pt→en**

`crates/core/tests/translate_ct2_pt.rs`:
```rust
//! GOW_MEET_OPUS_PT=$PWD/models/cand/<candidato> [GOW_MEET_OPUS_PT_PREFIX='>>eng<<'] cargo test -p gow-meet-core --release --test translate_ct2_pt -- --ignored --nocapture
use gow_meet_core::translate::{Ct2Translator, Translator};
use std::path::PathBuf;

#[test]
#[ignore]
fn translates_meeting_sentences_to_english() {
    let dir = PathBuf::from(std::env::var("GOW_MEET_OPUS_PT").expect("defina GOW_MEET_OPUS_PT"));
    let prefix = std::env::var("GOW_MEET_OPUS_PT_PREFIX").ok().filter(|s| !s.is_empty());
    let mut t = Ct2Translator::new(&dir, prefix).unwrap();
    let cases = [
        ("Vamos revisar o cronograma do projeto na próxima semana.", "week"),
        ("O novo painel precisa ficar pronto até sexta-feira.", "friday"),
        ("Você pode compartilhar a estimativa de orçamento com a equipe?", "budget"),
        ("Precisamos decidir quais funcionalidades entram na primeira versão.", "first"),
        ("Vou mandar um resumo depois desta reunião.", "summary"),
        ("Alguém tem alguma dúvida antes de seguirmos?", "question"),
        ("A gente pode marcar outra conversa na quinta à tarde.", "thursday"),
        ("O cliente pediu para adiar a entrega em duas semanas.", "two weeks"),
    ];
    let mut times = vec![];
    let mut misses = vec![];
    for (pt, must) in cases {
        let start = std::time::Instant::now();
        let en = t.translate(pt).unwrap();
        times.push(start.elapsed().as_millis());
        eprintln!("{pt} -> {en}  ({:?})", start.elapsed());
        if !en.to_lowercase().contains(must) { misses.push(format!("{en:?} sem {must:?}")); }
    }
    times.sort_unstable();
    eprintln!("tempo por frase: p50 {} ms, p90 {} ms", times[times.len() / 2], times[times.len() * 9 / 10]);
    assert!(misses.is_empty(), "{misses:#?}");
}
```

- [ ] **Step 5: Script de conversão com o modelo de origem como argumento**

Em `scripts/convert-opus-mt.sh`, troque a linha do `OUT` e a do conversor:
```bash
OUT="${1:-models/opus-mt-en-pt-ct2}"
SRC="${2:-Helsinki-NLP/opus-mt-tc-big-en-pt}"
```
```bash
ct2-transformers-converter --model "$SRC" \
  --output_dir "$OUT" --quantization int8 --copy_files source.spm target.spm
```
O padrão continua gerando o modelo en→pt de sempre.

- [ ] **Step 6: Medir o Whisper `small` multilíngue**

```bash
mkdir -p models/tmp
curl -L -o models/tmp/ggml-small-q5_1.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin
shasum -a 256 models/tmp/ggml-small-q5_1.bin
GOW_MEET_WHISPER_PT=$PWD/models/tmp/ggml-small-q5_1.bin \
  cargo test -p gow-meet-core --release --test asr_whisper_pt -- --ignored --nocapture
```
Expected: hash `ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb`; `portuguese_sentences_have_low_wer` passa (WER médio < 0,20); `silence_in_portuguese` imprime o bruto (anote as frases que sobrarem como "limpo=Some(...)": são as alucinações PT).

Latência em tempo real (precisa de um pt→en; use o primeiro candidato convertido no Step 7 e rode este comando depois dele):
```bash
cargo run -p gow-meet-cli --release -- crates/core/tests/fixtures/pt/meeting.wav --lang pt \
  --whisper models/tmp/ggml-small-q5_1.bin --opus models/cand/itc-en
```
Expected: linha `fala fechada → tradução: p50 … ms, p90 … ms` com p90 ≤ 3 000 ms.

Opcional (decide se a alternativa "modelo único" da spec seria viável): `GOW_MEET_WHISPER=$PWD/models/tmp/ggml-small-q5_1.bin cargo test -p gow-meet-core --release --test asr_whisper -- --ignored --nocapture` e compare o WER EN com o do `small.en`.

Regra: `small` aprovado se WER PT < 0,20 **e** p90 ≤ 3 s. Se reprovar, repita o Step 6 com `ggml-medium-q5_0.bin` (mesma revisão, hash `19fea4b380c3a618ec4723c3eef2eb785ffba0d0538cf43f8f235e7b3b34220f`); aprovado, o id no manifesto é `whisper-medium`. Se nenhum passar, pare e reporte BLOCKED com os números.

- [ ] **Step 7: Converter e medir os três candidatos pt→en**

```bash
scripts/convert-opus-mt.sh models/cand/itc-en Helsinki-NLP/opus-mt-itc-en
scripts/convert-opus-mt.sh models/cand/romance-en Helsinki-NLP/opus-mt-ROMANCE-en
scripts/convert-opus-mt.sh models/cand/bible-big Helsinki-NLP/opus-mt-tc-bible-big-itc-deu_eng_fra_por_spa
du -sh models/cand/*
for c in itc-en romance-en; do
  GOW_MEET_OPUS_PT=$PWD/models/cand/$c cargo test -p gow-meet-core --release --test translate_ct2_pt -- --ignored --nocapture
done
GOW_MEET_OPUS_PT=$PWD/models/cand/bible-big GOW_MEET_OPUS_PT_PREFIX='>>eng<<' \
  cargo test -p gow-meet-core --release --test translate_ct2_pt -- --ignored --nocapture
```
Se a conversão de algum candidato falhar, registre o erro em `docs/modelos-pt.md` e siga com os outros.

Regra de escolha, nesta ordem: (1) o teste passa (as 8 palavras-chave); (2) p90 por frase ≤ 300 ms; (3) entre os que sobram, o menor em disco, a menos que as traduções impressas de um maior sejam claramente melhores (diga por quê em uma linha). Se nenhum passar em (1), escolha o que errou menos e registre os erros.

- [ ] **Step 8: Registrar a medição em `docs/modelos-pt.md`**

Só números e decisões, sem trechos longos de saída:
```markdown
# Modelos para reuniões em português (etapa 3)

Máquina: Mac M2 8 GB · Data: <dd/mm/aaaa> · Commit: <sha>

## Transcrição (Whisper, idioma pt, frases fictícias da voz Luciana)

| Modelo | Tamanho | WER médio PT | Tempo por frase (s1..s5) | CLI p50 / p90 (fala → tradução) |
|---|---|---|---|---|
| ggml-small-q5_1 | 190 MB | … | … | … |

(EN com o `small` multilíngue, se medido: WER … contra … do `small.en`.)

## Tradução pt→en (CTranslate2 int8)

| Candidato | Revisão | Licença | Tamanho | Palavras-chave (8) | p50 / p90 por frase |
|---|---|---|---|---|---|
| opus-mt-itc-en | ce67164f | Apache-2.0 | … | …/8 | … |
| opus-mt-ROMANCE-en | e9ca9975 | Apache-2.0 | … | …/8 | … |
| opus-mt-tc-bible-big-itc-… (`>>eng<<`) | 4bcdcc25 | Apache-2.0 | … | …/8 | … |

## Escolha

- Whisper PT: `whisper-small` (ou `whisper-medium`, com o motivo).
- pt→en: `<candidato>`; prefixo: nenhum (ou `>>eng<<`). Motivo em uma linha.

## Alucinações

Frases que o Whisper devolveu em silêncio/ruído em português (minúsculas, exatamente como saíram depois de `clean_transcript`), uma por linha; "nenhuma" se não houve.
```

- [ ] **Step 9: PARE — pedir OK para publicar o modelo pt→en**

Publicar no Hugging Face é externo. Reporte ao controlador (status `NEEDS_CONTEXT`) com: candidato escolhido, tamanho, licença e o repositório proposto `gow-solucoes/opus-mt-pt-en-ct2` (público, como o `opus-mt-en-pt-ct2`). Só continue com o OK do usuário repassado pelo controlador.

Com o OK:
```bash
DIR=models/cand/<escolhido>
cat > "$DIR/README.md" <<'EOF'
---
license: apache-2.0
language: [pt, en]
tags: [translation, ctranslate2, opus-mt]
---
# opus-mt-pt-en (CTranslate2, int8)

Conversão para CTranslate2 (quantização int8) de `Helsinki-NLP/<candidato>` (revisão `<revisão>`), do projeto OPUS-MT (Universidade de Helsinque), licença Apache-2.0. Usado pelo gow-meet para traduzir reuniões do português para o inglês.

Conversão: `ct2-transformers-converter --model Helsinki-NLP/<candidato> --quantization int8 --copy_files source.spm target.spm`.
EOF
hf repo create gow-solucoes/opus-mt-pt-en-ct2 --type model || true
hf upload gow-solucoes/opus-mt-pt-en-ct2 "$DIR" .
curl -s https://huggingface.co/api/models/gow-solucoes/opus-mt-pt-en-ct2 | python3 -c "import json,sys; print(json.load(sys.stdin)['sha'])"
(cd "$DIR" && shasum -a 256 *)
```
O login do `hf` já existe nesta máquina (conta `gow-solucoes`); nunca mostre token.

- [ ] **Step 10: Manifesto**

Em `crates/core/models.json`, acrescente (antes de `"wespeaker-ecapa512-lm"`), com a revisão impressa pelo `curl` e os hashes do `shasum` (liste exatamente os arquivos que a conversão gerou; se houver `source_vocabulary.json`/`target_vocabulary.json` em vez de `shared_vocabulary.json`, use esses):
```json
  "whisper-small": {
    "files": [
      {
        "name": "ggml-small-q5_1.bin",
        "path": "ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin",
        "sha256": "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb"
      }
    ]
  },
  "opus-mt-pt-en": {
    "files": [
      { "name": "model.bin", "path": "gow-solucoes/opus-mt-pt-en-ct2/resolve/<revisão>/model.bin", "sha256": "<shasum>" },
      { "name": "config.json", "path": "gow-solucoes/opus-mt-pt-en-ct2/resolve/<revisão>/config.json", "sha256": "<shasum>" },
      { "name": "shared_vocabulary.json", "path": "gow-solucoes/opus-mt-pt-en-ct2/resolve/<revisão>/shared_vocabulary.json", "sha256": "<shasum>" },
      { "name": "source.spm", "path": "gow-solucoes/opus-mt-pt-en-ct2/resolve/<revisão>/source.spm", "sha256": "<shasum>" },
      { "name": "target.spm", "path": "gow-solucoes/opus-mt-pt-en-ct2/resolve/<revisão>/target.spm", "sha256": "<shasum>" }
    ]
  },
```
(Se o Whisper escolhido foi o `medium`, a entrada é `"whisper-medium"` com `ggml-medium-q5_0.bin` e o hash do Step 6.)

Em `crates/core/src/models.rs`, no teste `manifest_has_expected_ids_and_real_hashes`, a lista de ids passa a ser:
```rust
        for id in ["whisper-small-en", "whisper-turbo", "whisper-small", "opus-mt-en-pt", "opus-mt-pt-en",
                   "wespeaker-ecapa512-lm"] {
```
(com `whisper-medium` no lugar de `whisper-small`, se for o caso).

Confira o download pelo manifesto com o próprio código (baixa de novo do repositório publicado e confere os hashes):
```bash
GOW_MEET_MODELS_URL= cargo test -p gow-meet-core --lib manifest_has_expected_ids
```
Expected: `test result: ok. 1 passed`.

- [ ] **Step 11: README — licenças**

Na tabela "Modelos e licenças" do `README.md`, troque a linha do whisper.cpp e acrescente o pt→en:
```markdown
| whisper.cpp `small.en` / `small` (multilíngue) / `large-v3-turbo` | transcrição | MIT |
| Opus-MT en→pt (convertido para CTranslate2) | tradução | CC BY 4.0 |
| Opus-MT pt→en (`Helsinki-NLP/<candidato>`, convertido para CTranslate2) | tradução | Apache-2.0 |
```

- [ ] **Step 12: Commit**

```bash
git add scripts/gen-fixtures-pt.sh scripts/convert-opus-mt.sh crates/core/tests/fixtures/pt \
  crates/core/tests/asr_whisper_pt.rs crates/core/tests/translate_ct2_pt.rs crates/cli/src/main.rs \
  crates/core/models.json crates/core/src/models.rs docs/modelos-pt.md README.md
git commit -m "Mede os modelos em português e acrescenta Whisper multilíngue e Opus-MT pt→en ao manifesto" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 2: `Lang`, idioma em cada fala, alucinações por idioma e Markdown com separador

**Files:**
- Create: `crates/core/src/lang.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/asr.rs`, `crates/core/src/session.rs`, `crates/core/src/pipeline.rs`, `crates/core/tests/asr_whisper.rs`, `crates/core/tests/asr_whisper_pt.rs`, `src-tauri/src/lib.rs` (só literais de `Line` nos testes)

**Interfaces:**
- Consumes: seção **Alucinações** de `docs/modelos-pt.md` (Task 1).
- Produces:
  - `gow_meet_core::lang::Lang` (`En` padrão, `Pt`; serde `"en"`/`"pt"`; `Copy + Eq + Hash + Ord`), `Lang::ALL`, `code() -> &'static str`, `other() -> Lang`, `from_code(&str) -> Option<Lang>`, `name() -> &'static str` ("inglês"/"português").
  - `Line.lang: Lang` (`#[serde(default)]`).
  - `clean_transcript(raw: &str, lang: Lang) -> Option<String>`.
  - `SessionWriter::add_models(&mut self, models: BTreeMap<String, String>) -> Result<()>`.
  - Markdown: `*— agora em <nome> —*` entre falas quando o idioma muda.

- [ ] **Step 1: Teste de `Lang`**

`crates/core/src/lang.rs`:
```rust
//! Idiomas falados que o app entende (etapa 3). A tradução é sempre para o outro.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Pt,
}

#[cfg(test)]
mod tests {
    use super::Lang;

    #[test]
    fn codes_names_and_other() {
        assert_eq!(Lang::ALL, [Lang::En, Lang::Pt]);
        assert_eq!((Lang::En.code(), Lang::Pt.code()), ("en", "pt"));
        assert_eq!((Lang::En.other(), Lang::Pt.other()), (Lang::Pt, Lang::En));
        assert_eq!((Lang::from_code("pt"), Lang::from_code("en"), Lang::from_code("es")), (Some(Lang::Pt), Some(Lang::En), None));
        assert_eq!((Lang::En.name(), Lang::Pt.name()), ("inglês", "português"));
    }

    #[test]
    fn json_is_the_code() {
        assert_eq!(serde_json::to_string(&Lang::Pt).unwrap(), "\"pt\"");
        assert_eq!(serde_json::from_str::<Lang>("\"en\"").unwrap(), Lang::En);
        assert_eq!(Lang::default(), Lang::En);
    }
}
```
Em `crates/core/src/lib.rs`, acrescente `pub mod lang;` (em ordem alfabética, depois de `fbank`).

Run: `cargo test -p gow-meet-core --lib lang::`
Expected: FAIL de compilação (`no function or associated item named code`…).

- [ ] **Step 2: Implementar `Lang`**

Em `lang.rs`, depois do `enum`:
```rust
impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Pt];

    /// Código ISO 639-1: idioma do Whisper e valor no JSON.
    pub fn code(self) -> &'static str {
        match self { Lang::En => "en", Lang::Pt => "pt" }
    }

    /// Idioma da tradução.
    pub fn other(self) -> Lang {
        match self { Lang::En => Lang::Pt, Lang::Pt => Lang::En }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        match s { "en" => Some(Lang::En), "pt" => Some(Lang::Pt), _ => None }
    }

    /// Nome em português, minúsculo, para avisos e o Markdown.
    pub fn name(self) -> &'static str {
        match self { Lang::En => "inglês", Lang::Pt => "português" }
    }
}
```
Run: `cargo test -p gow-meet-core --lib lang::`
Expected: `2 passed`.

- [ ] **Step 3: Testes do filtro por idioma**

Em `crates/core/src/asr.rs`, no módulo de testes, troque o `use` e os testes de `clean_transcript` para passar o idioma, e acrescente um teste:
```rust
    use super::{clean_transcript, pad_to_min};
    use crate::lang::Lang;
    #[test]
    fn keeps_normal_speech() {
        assert_eq!(clean_transcript("  Let's review the timeline. ", Lang::En).as_deref(), Some("Let's review the timeline."));
        assert_eq!(clean_transcript(" Vamos revisar o cronograma. ", Lang::Pt).as_deref(), Some("Vamos revisar o cronograma."));
    }
    #[test]
    fn drops_markers_and_hallucinations() {
        for raw in ["[BLANK_AUDIO]", " (music) ", "*laughs*", "Thanks for watching!", "[ Silence ]", ""] {
            assert_eq!(clean_transcript(raw, Lang::En), None, "{raw:?}");
        }
    }
    #[test]
    fn portuguese_hallucinations_are_dropped_in_portuguese() {
        for raw in ["Legendas pela comunidade Amara.org", "Obrigado por assistir!", "[Música]"] {
            assert_eq!(clean_transcript(raw, Lang::Pt), None, "{raw:?}");
        }
    }
    #[test]
    fn strips_markers_inside_text() {
        assert_eq!(clean_transcript("[MUSIC] Good morning everyone.", Lang::En).as_deref(), Some("Good morning everyone."));
    }
```
Run: `cargo test -p gow-meet-core --lib asr::`
Expected: FAIL de compilação (`clean_transcript` recebe 1 argumento).

- [ ] **Step 4: Alucinações por idioma**

Em `asr.rs`, troque a constante `HALLUCINATIONS` e a assinatura:
```rust
use crate::lang::Lang;

/// Frases que o Whisper inventa em silêncio, por idioma (minúsculas, comparação exata).
const HALLUCINATIONS_EN: &[&str] = &[
    "thanks for watching!", "thank you for watching.", "thank you for watching!",
    "subtitles by the amara.org community",
];
/// Português: conhecidas do Whisper e as da medição (`docs/modelos-pt.md`, seção Alucinações).
const HALLUCINATIONS_PT: &[&str] = &[
    "legendas pela comunidade amara.org", "obrigado por assistir.", "obrigado por assistir!",
    "obrigado por assistir",
];

fn hallucinations(lang: Lang) -> &'static [&'static str] {
    match lang { Lang::En => HALLUCINATIONS_EN, Lang::Pt => HALLUCINATIONS_PT }
}
```
Acrescente a `HALLUCINATIONS_PT` cada frase da seção **Alucinações** de `docs/modelos-pt.md` que ainda não esteja na lista. Em `clean_transcript`:
```rust
pub fn clean_transcript(raw: &str, lang: Lang) -> Option<String> {
```
```rust
    if text.is_empty() || hallucinations(lang).contains(&text.to_lowercase().as_str()) { return None; }
```
Atualize os chamadores:
- `crates/core/src/pipeline.rs`: `let Some(text) = clean_transcript(&raw, crate::lang::Lang::En) else {` (a Task 3 troca por `job.lang`).
- `crates/core/tests/asr_whisper.rs`: `use gow_meet_core::lang::Lang;` e `clean_transcript(&…, Lang::En)` nas três chamadas.
- `crates/core/tests/asr_whisper_pt.rs`: `use gow_meet_core::lang::Lang;` e `clean_transcript(&…, Lang::Pt)` nas duas chamadas.

Run: `cargo test -p gow-meet-core --lib asr::`
Expected: `5 passed`.

- [ ] **Step 5: Testes de sessão com idioma**

Em `crates/core/src/session.rs`, no módulo de testes, o helper `line` ganha `lang: Lang::En` e entram três testes:
```rust
    use crate::lang::Lang;

    fn line(id: &str, t0: u64, text: &str, tr: Option<&str>) -> Line {
        Line { id: id.into(), t0, t1: t0 + 500, speaker: Speaker::system(), lang: Lang::En,
               text: text.into(), translation: tr.map(Into::into), translation_error: false }
    }

    #[test]
    fn line_without_lang_is_english_and_lang_is_always_written() {
        let old = r#"{"id":"s0","t0":0,"t1":500,"speaker":{"source":"system","label":"Participante"},"text":"Hello."}"#;
        let l: Line = serde_json::from_str(old).unwrap();
        assert_eq!(l.lang, Lang::En);
        let pt = Line { lang: Lang::Pt, ..line("s1", 0, "Olá.", None) };
        assert_eq!(serde_json::to_value(&pt).unwrap()["lang"], "pt");
        assert_eq!(serde_json::to_value(&l).unwrap()["lang"], "en");
    }

    #[test]
    fn markdown_marks_language_changes() {
        let lines = vec![
            line("s0", 0, "Hello team.", Some("Olá, equipe.")),
            Line { lang: Lang::Pt, ..line("s1", 1_000, "Bom dia.", Some("Good morning.")) },
            Line { lang: Lang::Pt, ..line("m0", 2_000, "Vamos começar.", Some("Let's start.")) },
            line("s2", 3_000, "Thanks.", Some("Obrigado.")),
        ];
        let md = render_markdown(&meta("x"), &lines, &Speakers::new());
        assert_eq!(md.matches("*— agora em português —*").count(), 1);
        assert_eq!(md.matches("*— agora em inglês —*").count(), 1);
        let (a, b) = (md.find("Olá, equipe.").unwrap(), md.find("agora em português").unwrap());
        assert!(a < b && b < md.find("Good morning.").unwrap());
        // Reunião de um idioma só: sem separador.
        let one = render_markdown(&meta("y"), &lines[..1], &Speakers::new());
        assert!(!one.contains("agora em"));
    }

    #[test]
    fn add_models_rewrites_meta() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("s9")).unwrap();
        w.add_models([("asr_pt".to_string(), "whisper-small".to_string())].into()).unwrap();
        let m: Meta = serde_json::from_slice(&std::fs::read(w.dir().join("meta.json")).unwrap()).unwrap();
        assert_eq!(m.models.get("asr_pt").map(String::as_str), Some("whisper-small"));
        let md = w.finish(2_000).unwrap();
        let m: Meta = serde_json::from_slice(&std::fs::read(md.parent().unwrap().join("meta.json")).unwrap()).unwrap();
        assert_eq!(m.models.get("asr_pt").map(String::as_str), Some("whisper-small"));
    }
```
Run: `cargo test -p gow-meet-core --lib session::`
Expected: FAIL de compilação (`Line` sem `lang`; `add_models` inexistente).

- [ ] **Step 6: `Line.lang`, `add_models` e separador**

Em `session.rs`:
```rust
use crate::lang::Lang;
```
Em `Line`, depois de `speaker`:
```rust
    /// Idioma falado (etapa 3); a tradução é o outro. Linhas antigas, sem o campo: inglês.
    #[serde(default)]
    pub lang: Lang,
```
Em `impl SessionWriter`, depois de `set_speakers`:
```rust
    /// Registra modelos carregados depois do início (o par do outro idioma) e regrava o `meta.json`.
    pub fn add_models(&mut self, models: BTreeMap<String, String>) -> Result<()> {
        self.meta.models.extend(models);
        write_meta(&self.dir, &self.meta)
    }
```
Em `render_markdown`, o laço passa a ser:
```rust
    let mut prev: Option<Lang> = None;
    for l in lines {
        if prev.is_some_and(|p| p != l.lang) {
            out.push_str(&format!("*— agora em {} —*\n\n", l.lang.name()));
        }
        prev = Some(l.lang);
        let tr = l.translation.clone().unwrap_or_else(|| "_(sem tradução)_".into());
        out.push_str(&format!("**[{}] {}:** {}\n> {}\n\n", clock(l.t0), label_for(l, speakers), tr, l.text));
    }
```
Acrescente `lang: Lang::En` (ou `lang: crate::lang::Lang::En`) a todo literal `Line { … }` restante: os demais testes de `session.rs`, a criação da fala final em `pipeline.rs` (`asr_loop`; a Task 3 troca por `job.lang`), e os três literais nos testes de `src-tauri/src/lib.rs` (`use gow_meet_core::lang::Lang;` no módulo de testes). Use `grep -rn 'Line {' crates src-tauri/src` para achar todos.

Run:
```bash
cargo test -p gow-meet-core --lib
cargo test -p gow-meet-app --lib
cargo build -p gow-meet-cli
```
Expected: tudo passa (core: os testes de antes + 6 novos); o CLI compila.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/lang.rs crates/core/src/lib.rs crates/core/src/asr.rs crates/core/src/session.rs \
  crates/core/src/pipeline.rs crates/core/tests/asr_whisper.rs crates/core/tests/asr_whisper_pt.rs src-tauri/src/lib.rs
git commit -m "Idioma em cada fala, alucinações por idioma e separador de idioma no Markdown" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 3: Troca de idioma no pipeline

**Files:**
- Modify: `crates/core/src/pipeline.rs`, `crates/cli/src/main.rs`
- Create: `crates/core/tests/lang_switch.rs`

**Interfaces:**
- Consumes: `Lang`, `Line.lang`, `clean_transcript(raw, lang)`, `SessionWriter::add_models` (Task 2).
- Produces (usados pela Task 4):
  - `PipelineConfig.lang: Lang` (padrão `En`): idioma dos modelos passados a `start`/`start_with_speakers` (assinaturas inalteradas).
  - `AsrJob.lang: Lang`.
  - `Pipeline::add_language(&self, lang: Lang, asr: Box<dyn Transcriber>, tr: Box<dyn Translator>)`.
  - `Pipeline::set_language(&self, lang: Lang) -> bool` (`false` se já era o idioma).
  - `Pipeline::language(&self) -> Lang`.
  - `Pipeline::add_models(&self, models: BTreeMap<String, String>)`.
  - `PipelineEvent::Partial { id, speaker, t0, t1, text, translation, lang }`.

- [ ] **Step 1: Testes da troca**

No módulo de testes de `pipeline.rs`, acrescente (o helper `job` ganha `lang: Lang::En`):
```rust
    use crate::lang::Lang;

    struct ToEn;
    impl Translator for ToEn {
        fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("en: {t}")) }
    }
    fn spoken() -> Vec<f32> { [tone(1_500), silence(1_000)].concat() }
    fn add_pt(p: &Pipeline) { p.add_language(Lang::Pt, Box::new(FakeAsr("Olá, equipe.")), Box::new(ToEn)); }
    fn summary(lines: &[Line]) -> Vec<(String, Lang, String, Option<String>)> {
        lines.iter().map(|l| (l.id.clone(), l.lang, l.text.clone(), l.translation.clone())).collect()
    }

    #[test]
    fn audio_before_switch_stays_in_old_language() {
        let (_d, ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &spoken());
            add_pt(p);
            assert!(p.set_language(Lang::Pt));
            assert_eq!(p.language(), Lang::Pt);
            p.feed(SpeakerSource::System, &spoken());
        });
        assert_eq!(summary(&lines), vec![
            ("s0".into(), Lang::En, "Hello team.".into(), Some("pt: Hello team.".into())),
            ("s1".into(), Lang::Pt, "Olá, equipe.".into(), Some("en: Olá, equipe.".into())),
        ]);
        let partials: Vec<(String, Lang)> = ev.iter().filter_map(|e| match e {
            PipelineEvent::Partial { id, lang, .. } => Some((id.clone(), *lang)), _ => None }).collect();
        assert!(partials.contains(&("s1".into(), Lang::Pt)), "{partials:?}");
        assert!(partials.iter().all(|(id, l)| (id == "s0") == (*l == Lang::En)), "{partials:?}");
    }

    #[test]
    fn switch_closes_open_utterances_in_old_language() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &tone(1_500)); // fala aberta
            add_pt(p);
            p.set_language(Lang::Pt);
            p.feed(SpeakerSource::System, &spoken());
        });
        assert_eq!(summary(&lines), vec![
            ("s0".into(), Lang::En, "Hello team.".into(), Some("pt: Hello team.".into())),
            ("s1".into(), Lang::Pt, "Olá, equipe.".into(), Some("en: Olá, equipe.".into())),
        ]);
    }

    #[test]
    fn both_sources_switch_together() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::Mic, &spoken());
            p.feed(SpeakerSource::System, &spoken());
            add_pt(p);
            p.set_language(Lang::Pt);
            p.feed(SpeakerSource::Mic, &spoken());
            p.feed(SpeakerSource::System, &spoken());
        });
        let mut got: Vec<(String, Lang)> = lines.iter().map(|l| (l.id.clone(), l.lang)).collect();
        got.sort();
        assert_eq!(got, vec![("m0".into(), Lang::En), ("m1".into(), Lang::Pt), ("s0".into(), Lang::En), ("s1".into(), Lang::Pt)]);
    }

    #[test]
    fn same_language_is_a_no_op() {
        let (_d, _ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.feed(SpeakerSource::System, &tone(1_500));
            assert!(!p.set_language(Lang::En));
            p.feed(SpeakerSource::System, &spoken()); // mesma fala continua
        });
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].lang, Lang::En);
    }

    #[test]
    fn language_without_models_drops_lines() {
        let (_d, ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.set_language(Lang::Pt);
            p.feed(SpeakerSource::System, &spoken());
        });
        assert!(lines.is_empty());
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(_))));
    }

    #[test]
    fn panic_in_one_language_keeps_the_other() {
        let (_d, ev, lines) = run("Hello team.", Box::new(Prefix), |p| {
            p.add_language(Lang::Pt, Box::new(PanicAsr), Box::new(ToEn));
            p.set_language(Lang::Pt);
            p.feed(SpeakerSource::System, &spoken());
            p.feed(SpeakerSource::System, &spoken());
            p.set_language(Lang::En);
            p.feed(SpeakerSource::System, &spoken());
        });
        assert_eq!(lines.iter().map(|l| l.lang).collect::<Vec<_>>(), vec![Lang::En]);
        let panics = ev.iter().filter(|e| matches!(e, PipelineEvent::Error(m) if m == ASR_PANIC_MESSAGE)).count();
        assert_eq!(panics, 1);
    }

    #[test]
    fn add_models_goes_to_meta() {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let p = Pipeline::start(PipelineConfig::default(), Box::new(FakeAsr("Hi.")), Box::new(Prefix), writer,
                                Arc::new(Collect::default()));
        p.add_models([("asr_pt".to_string(), "whisper-small".to_string())].into());
        let md = p.finish(1).unwrap();
        let m: Meta = serde_json::from_slice(&std::fs::read(md.parent().unwrap().join("meta.json")).unwrap()).unwrap();
        assert_eq!(m.models.get("asr_pt").map(String::as_str), Some("whisper-small"));
    }

    #[test]
    fn coalesce_keeps_jobs_of_each_language() {
        use SpeakerSource::*;
        let pt = |mut j: AsrJob| { j.lang = Lang::Pt; j };
        let out = coalesce(vec![job(System, 0, false, 1_000), job(System, 0, true, 1_500), pt(job(System, 1, false, 2_500))]);
        let keys: Vec<_> = out.iter().map(|j| (j.seq, j.is_final, j.lang)).collect();
        assert_eq!(keys, vec![(0, true, Lang::En), (1, false, Lang::Pt)]);
    }
```
(`PanicAsr` já existe no módulo de testes.)

Run: `cargo test -p gow-meet-core --lib pipeline::`
Expected: FAIL de compilação (`add_language`, `set_language`, `language`, `add_models`, `AsrJob.lang`, `Partial.lang`).

- [ ] **Step 2: Tipos: idioma na configuração, no job, na parcial e na fila**

Em `pipeline.rs`:
```rust
use crate::lang::Lang;
use std::collections::BTreeMap;
```
`PipelineEvent::Partial` ganha `lang: Lang` (último campo). `PipelineConfig` ganha:
```rust
    /// Idioma falado no início: os modelos passados a `start` são deste idioma.
    pub lang: Lang,
```
com `lang: Lang::En` no `Default`. `AsrJob` ganha `pub lang: Lang` (último campo). Acrescente:
```rust
/// Fila da transcrição: falas, ou os modelos de mais um idioma (que chegam antes de
/// qualquer fala carimbada com ele, por virem na mesma fila).
enum AsrMsg {
    Job(AsrJob),
    Load { lang: Lang, asr: Box<dyn Transcriber>, tr: Box<dyn Translator> },
}
```
`TJob` passa a ser:
```rust
enum TJob {
    Final(Line),
    Partial { id: String, speaker: Speaker, t0: u64, t1: u64, text: String, lang: Lang },
    /// Tradutor de mais um idioma, repassado pela thread de transcrição.
    Load(Lang, Box<dyn Translator>),
}
```
Segmentadores e idioma corrente ficam sob o mesmo lock:
```rust
/// Segmentadores e idioma corrente sob o mesmo lock: cada fala sai carimbada com o
/// idioma que valia quando foi enviada, na ordem da fila.
struct Feed { segmenters: HashMap<SpeakerSource, Segmenter>, lang: Lang }
```
Em `Pipeline`, `segmenters: Mutex<HashMap<…>>` vira `feed: Mutex<Feed>`, e `asr_tx: Option<Sender<AsrJob>>` vira `Option<Sender<AsrMsg>>`.

- [ ] **Step 3: Envio carimbado, `set_language`, `add_language`, `language`, `add_models`**

Em `start_with_speakers`: `let (asr_tx, asr_rx) = unbounded::<AsrMsg>();`, `let lang = cfg.lang;` antes de criar as threads; a thread `asr` recebe `HashMap::from([(lang, asr)])` e a `translate` recebe `HashMap::from([(lang, translator)])`; o `Self` usa `feed: Mutex::new(Feed { segmenters: HashMap::new(), lang })`.

Substitua `feed`, `send` e o começo de `finish`, e acrescente os métodos novos:
```rust
    pub fn feed(&self, source: SpeakerSource, samples: &[f32]) {
        let mut feed = self.feed.lock().unwrap();
        let lang = feed.lang;
        let seg = feed.segmenters.entry(source).or_insert_with(|| Segmenter::new(
            self.cfg.segmenter.clone(), Box::new(RmsDetector { threshold: self.cfg.vad_threshold })));
        let events = seg.push(samples);
        *self.fed.lock().unwrap().entry(source).or_insert(0) += samples.len() as u64;
        // Enviado com o lock: nenhuma fala anterior a uma troca sai com o idioma novo.
        self.send(source, lang, events);
    }

    /// Idioma em que as próximas falas serão transcritas.
    pub fn language(&self) -> Lang { self.feed.lock().unwrap().lang }

    /// Entrega às threads os modelos de mais um idioma, pela própria fila: chegam antes de
    /// qualquer fala desse idioma. Não troca o idioma (veja `set_language`).
    pub fn add_language(&self, lang: Lang, asr: Box<dyn Transcriber>, tr: Box<dyn Translator>) {
        if let Some(tx) = &self.asr_tx { let _ = tx.send(AsrMsg::Load { lang, asr, tr }); }
    }

    /// Troca o idioma falado: fecha as falas abertas das duas fontes no idioma antigo, e o
    /// áudio seguinte sai no novo. `false` se já era esse o idioma. Sem `add_language` antes,
    /// as falas do idioma novo são descartadas.
    pub fn set_language(&self, lang: Lang) -> bool {
        let mut feed = self.feed.lock().unwrap();
        if feed.lang == lang { return false; }
        let old = feed.lang;
        let flushed: Vec<(SpeakerSource, Vec<SegmentEvent>)> =
            feed.segmenters.iter_mut().map(|(s, seg)| (*s, seg.flush())).collect();
        for (s, ev) in flushed { self.send(s, old, ev); }
        feed.lang = lang;
        tracing::info!(de = old.code(), para = lang.code(), "idioma trocado");
        true
    }

    /// Modelos carregados depois do início (o par do outro idioma), para o `meta.json`.
    pub fn add_models(&self, models: BTreeMap<String, String>) {
        if let Some(s) = self.store.lock().unwrap().as_mut() {
            if let Err(e) = s.writer.add_models(models) { tracing::warn!("meta.json não atualizado: {e:#}"); }
        }
    }

    fn send(&self, source: SpeakerSource, lang: Lang, events: Vec<SegmentEvent>) {
        let Some(tx) = &self.asr_tx else { return };
        for ev in events {
            let job = match ev {
                SegmentEvent::Partial { seq, start_ms, audio } => {
                    let end_ms = start_ms + audio.len() as u64 / 16;
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: false, lang }
                }
                SegmentEvent::Final { seq, start_ms, end_ms, audio } =>
                    AsrJob { source, seq, start_ms, end_ms, audio, is_final: true, lang },
            };
            let _ = tx.send(AsrMsg::Job(job));
        }
    }
```
Em `finish`, o começo vira:
```rust
        {
            let mut feed = self.feed.lock().unwrap();
            let lang = feed.lang;
            let flushed: Vec<(SpeakerSource, Vec<SegmentEvent>)> =
                feed.segmenters.iter_mut().map(|(s, seg)| (*s, seg.flush())).collect();
            for (s, ev) in flushed { self.send(s, lang, ev); }
        }
        self.asr_tx.take();
```
Ordem de locks nova: `feed` → `fed` (a thread de transcrição só pega `fed`, sozinho).

- [ ] **Step 4: Thread de transcrição com um modelo por idioma**

Em `asr_loop`, a assinatura troca `rx: Receiver<AsrJob>, mut asr: Box<dyn Transcriber>` por `rx: Receiver<AsrMsg>, mut models: HashMap<Lang, Box<dyn Transcriber>>`, e a variável `dead` sai. O começo do laço:
```rust
    while let Ok(first) = rx.recv() {
        let mut batch = vec![];
        for msg in std::iter::once(first).chain(rx.try_iter()) {
            match msg {
                // Modelos novos valem já: só falas carimbadas com esse idioma os usam.
                AsrMsg::Load { lang, asr, tr } => {
                    models.insert(lang, asr);
                    let _ = tr_tx.send(TJob::Load(lang, tr));
                }
                AsrMsg::Job(j) => batch.push(j),
            }
        }
        for job in coalesce(batch) {
```
No lugar do bloco `if dead { … }` e do `match guarded(|| asr.transcribe(…))`:
```rust
            // Idioma sem modelo (desligado por pânico, ou nunca carregado): segue drenando a fila.
            let Some(asr) = models.get_mut(&job.lang) else {
                drop_final(sink.as_ref());
                continue;
            };
            let raw = match guarded(|| asr.transcribe(&job.audio)) {
                None => {
                    models.remove(&job.lang);
                    tracing::error!(lang = job.lang.code(), "pânico na transcrição; esse idioma fica desligado nesta sessão");
                    sink.emit(PipelineEvent::Error(ASR_PANIC_MESSAGE.into()));
                    drop_final(sink.as_ref());
                    continue;
                }
```
(o resto do `match` fica igual). Depois: `clean_transcript(&raw, job.lang)`; a fala final é criada com `lang: job.lang`; a parcial emite `PipelineEvent::Partial { …, translation: None, lang: job.lang }` e envia `TJob::Partial { id, speaker, t0: job.start_ms, t1: job.end_ms, text, lang: job.lang }`.

- [ ] **Step 5: Thread de tradução com um modelo por idioma**

Troque a closure `translate` por uma função, para o mapa poder receber modelos novos:
```rust
/// Traduz com o modelo do idioma da fala, isolando pânicos: o primeiro desliga a tradução
/// desse idioma e avisa uma vez. `None`: idioma sem tradutor (desligado ou não carregado).
fn translate_with(models: &mut HashMap<Lang, Box<dyn Translator>>, lang: Lang, text: &str, sink: &dyn Sink)
    -> Option<Result<String>> {
    let tr = models.get_mut(&lang)?;
    let r = guarded(|| tr.translate(text));
    if r.is_none() {
        models.remove(&lang);
        tracing::error!(lang = lang.code(), "pânico na tradução; tradução desse idioma desligada nesta sessão");
        sink.emit(PipelineEvent::Error(TRANSLATE_PANIC_MESSAGE.into()));
    }
    r
}
```
`translate_loop` recebe `mut models: HashMap<Lang, Box<dyn Translator>>` no lugar de `mut tr`, sem `dead`. No começo de cada lote:
```rust
        let mut batch = vec![];
        for job in std::iter::once(first).chain(rx.try_iter()) {
            match job {
                TJob::Load(lang, tr) => { models.insert(lang, tr); }
                other => batch.push(other),
            }
        }
```
Finais: `match translate_with(&mut models, line.lang, &line.text, sink.as_ref()) {` (mesmos três braços). Parciais: o padrão vira `TJob::Partial { id, speaker, t0, t1, text, lang } = job` e a tradução `translate_with(&mut models, lang, &text, sink.as_ref())`; o evento sai com `lang`. O braço `TJob::Load(..)` não aparece mais dentro do laço do lote (já foi tirado); nos `match` restantes use `TJob::Load(..) => {}` onde o compilador exigir.

Run: `cargo test -p gow-meet-core --lib`
Expected: todos passam (os de antes e os 8 novos). Se `audio_before_switch_stays_in_old_language` falhar só na asserção das parciais, confira que a parcial carrega `job.lang`, não o idioma corrente.

- [ ] **Step 6: CLI com `Lang`**

Em `crates/cli/src/main.rs`, depois de `let a = Args::parse();`:
```rust
    let lang = gow_meet_core::lang::Lang::from_code(&a.lang).context("--lang deve ser en ou pt")?;
    let cfg = PipelineConfig { lang, ..PipelineConfig::default() };
```
(remova o `let cfg = PipelineConfig::default();` antigo); `WhisperTranscriber::new(&a.whisper, lang.code(), !a.cpu)`; no `Meta`, `source_lang: lang.code().into(), target_lang: lang.other().code().into()`.

Run: `cargo build -p gow-meet-cli --release && cargo test -p gow-meet-cli`
Expected: compila; testes do CLI passam.

- [ ] **Step 7: Teste de integração com os modelos reais (ignorado)**

`crates/core/tests/lang_switch.rs`:
```rust
//! Troca de idioma com os modelos reais (frases fictícias do `say`). Rode com:
//! GOW_MEET_WHISPER=… GOW_MEET_OPUS=… GOW_MEET_OPUS_PREFIX='>>pob<<' \
//! GOW_MEET_WHISPER_PT=… GOW_MEET_OPUS_PT=… [GOW_MEET_OPUS_PT_PREFIX=…] \
//! cargo test -p gow-meet-core --release --test lang_switch -- --ignored --nocapture
use gow_meet_core::asr::WhisperTranscriber;
use gow_meet_core::lang::Lang;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink};
use gow_meet_core::session::{replay, Meta, SessionWriter, SpeakerSource};
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::wav::read_wav_16k_mono;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Quiet;
impl Sink for Quiet { fn emit(&self, _ev: PipelineEvent) {} }

fn env(k: &str) -> PathBuf { PathBuf::from(std::env::var(k).unwrap_or_else(|_| panic!("defina {k}"))) }
fn prefix(k: &str) -> Option<String> { std::env::var(k).ok().filter(|s| !s.is_empty()) }

#[test]
#[ignore]
fn english_then_portuguese_in_one_meeting() {
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "troca".into(), title: "Teste".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let writer = SessionWriter::create(root.path(), meta).unwrap();
    let p = Pipeline::start(PipelineConfig::default(),
        Box::new(WhisperTranscriber::new(&env("GOW_MEET_WHISPER"), "en", true).unwrap()),
        Box::new(Ct2Translator::new(&env("GOW_MEET_OPUS"), prefix("GOW_MEET_OPUS_PREFIX")).unwrap()),
        writer, Arc::new(Quiet));
    let silence = vec![0.0f32; 16_000];
    p.feed(SpeakerSource::System, &read_wav_16k_mono(&fx.join("s1.wav")).unwrap());
    p.feed(SpeakerSource::System, &silence);
    p.add_language(Lang::Pt,
        Box::new(WhisperTranscriber::new(&env("GOW_MEET_WHISPER_PT"), "pt", true).unwrap()),
        Box::new(Ct2Translator::new(&env("GOW_MEET_OPUS_PT"), prefix("GOW_MEET_OPUS_PT_PREFIX")).unwrap()));
    assert!(p.set_language(Lang::Pt));
    p.feed(SpeakerSource::System, &read_wav_16k_mono(&fx.join("pt/s1.wav")).unwrap());
    p.feed(SpeakerSource::System, &silence);
    let md = p.finish(60_000).unwrap();
    let lines = replay(&md.parent().unwrap().join("lines.jsonl")).unwrap();
    for l in &lines { eprintln!("{} {:?} | {} -> {:?}", l.id, l.lang, l.text, l.translation); }
    assert_eq!(lines.iter().map(|l| l.lang).collect::<Vec<_>>(), vec![Lang::En, Lang::Pt]);
    assert!(lines[0].translation.as_deref().unwrap_or("").to_lowercase().contains("semana"));
    assert!(lines[1].translation.as_deref().unwrap_or("").to_lowercase().contains("week"));
    assert!(std::fs::read_to_string(&md).unwrap().contains("*— agora em português —*"));
}
```
Run (com os modelos de `models/tmp` e `models/cand` da Task 1):
```bash
GOW_MEET_WHISPER=$PWD/models/tmp/ggml-small.en-q5_1.bin GOW_MEET_OPUS=$PWD/models/opus-mt-en-pt-ct2 GOW_MEET_OPUS_PREFIX='>>pob<<' \
GOW_MEET_WHISPER_PT=$PWD/models/tmp/ggml-small-q5_1.bin GOW_MEET_OPUS_PT=$PWD/models/cand/<escolhido> \
  cargo test -p gow-meet-core --release --test lang_switch -- --ignored --nocapture
```
Se algum caminho não existir, use os modelos baixados pelo app em `~/Library/Application Support/gow-meet/models/<id>/`. Expected: `1 passed`.

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/pipeline.rs crates/core/tests/lang_switch.rs crates/cli/src/main.rs
git commit -m "Pipeline troca de idioma no meio da reunião sem misturar idiomas numa fala" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 4: App — iniciar com idioma, pré-carregar, `set_language` e evento `language`

**Files:**
- Create: `src-tauri/src/language.rs`
- Modify: `src-tauri/src/config.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/main.json`, `src-tauri/capabilities/overlay.json`, `src-tauri/permissions/autogenerated/` (gerado), `README.md`

**Interfaces:**
- Consumes: `Lang`, `PipelineConfig.lang`, `Pipeline::{add_language, set_language, add_models}`, `PipelineEvent::Partial.lang` (Task 3); ids e prefixo da seção **Escolha** de `docs/modelos-pt.md` (Task 1).
- Produces (usados pela Task 5):
  - comandos `ensure_models {lang}`, `start_session {title, lang}`, `set_language {lang}`;
  - `UiEvent::Language { lang, status, progress? }` → JSON `{"kind":"language","lang":"pt","status":"preparing","progress":0.4}` (sem `progress` quando ausente);
  - `UiEvent::Partial` com `lang`.

- [ ] **Step 1: Testes da configuração**

Em `src-tauri/src/config.rs`, troque o teste `partial_override_keeps_other_defaults` (a asserção de `target_lang` sai) e acrescente dois:
```rust
    #[test]
    fn partial_override_keeps_other_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"whisper_model":"whisper-turbo","models_base_url":"http://espelho.local"}"#).unwrap();
        let c = Config::load(&p);
        assert_eq!(c.whisper_model, "whisper-turbo");
        assert_eq!(c.models_base_url, "http://espelho.local");
        assert_eq!(c.opus_model, "opus-mt-en-pt");
        assert_eq!(c.whisper_model_pt, Config::default().whisper_model_pt);
    }

    #[test]
    fn pair_for_each_language() {
        let c = Config::default();
        assert_eq!(c.pair(Lang::En), Pair { whisper: "whisper-small-en".into(), opus: "opus-mt-en-pt".into(),
                                            prefix: Some(">>pob<<".into()) });
        let pt = c.pair(Lang::Pt);
        assert_eq!((pt.whisper.as_str(), pt.opus.as_str()), (c.whisper_model_pt.as_str(), "opus-mt-pt-en"));
        assert_eq!(pt.prefix, c.opus_prefix_pt);
    }

    #[test]
    fn old_config_with_language_keys_still_loads() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"source_lang":"en","target_lang":"pt","whisper_model":"whisper-turbo"}"#).unwrap();
        assert_eq!(Config::load(&p).whisper_model, "whisper-turbo");
    }
```
Run: `cargo test -p gow-meet-app --lib config::`
Expected: FAIL de compilação (`whisper_model_pt`, `pair`, `Pair`).

- [ ] **Step 2: Chaves PT e `Config::pair`**

Em `config.rs`: `use gow_meet_core::lang::Lang;`. No `Config`, remova `source_lang` e `target_lang` e acrescente (depois de `opus_prefix`):
```rust
    /// Português: Whisper multilíngue e tradução para o inglês (ids do manifesto).
    pub whisper_model_pt: String,
    pub opus_model_pt: String,
    pub opus_prefix_pt: Option<String>,
```
No `Default`, remova as duas linhas de idioma e acrescente, com os valores da seção **Escolha** de `docs/modelos-pt.md`:
```rust
            whisper_model_pt: "whisper-small".into(),
            opus_model_pt: "opus-mt-pt-en".into(),
            opus_prefix_pt: None, // Some(">>eng<<".into()) se o escolhido foi o tc-bible-big
```
Comente as chaves EN como "Inglês: transcrição e tradução para o português". Acrescente:
```rust
/// Modelos de um idioma falado (ids do manifesto) e o prefixo de destino do Opus-MT.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair { pub whisper: String, pub opus: String, pub prefix: Option<String> }

impl Config {
    pub fn pair(&self, lang: Lang) -> Pair {
        match lang {
            Lang::En => Pair { whisper: self.whisper_model.clone(), opus: self.opus_model.clone(),
                               prefix: self.opus_prefix.clone() },
            Lang::Pt => Pair { whisper: self.whisper_model_pt.clone(), opus: self.opus_model_pt.clone(),
                               prefix: self.opus_prefix_pt.clone() },
        }
    }
}
```
(no mesmo `impl Config` de `tracker()`/`load()`). Run: `cargo test -p gow-meet-app --lib config::` → os 6 testes passam (o `lib.rs` ainda não compila se usar `cfg.source_lang`: siga para o Step 4 antes de rodar o crate inteiro, ou rode só depois do Step 6).

- [ ] **Step 3: Testes do estado do idioma**

`src-tauri/src/language.rs`:
```rust
//! Idioma de uma reunião em andamento (etapa 3): o que vale no pipeline, o último pedido
//! do usuário e os pares de modelos já entregues ao pipeline ou em preparo.
use gow_meet_core::lang::Lang;
use std::collections::HashSet;

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
```
Em `lib.rs`, acrescente `mod language;` (depois de `mod config;`).

Run: `cargo test -p gow-meet-app --lib language::`
Expected: FAIL de compilação (`LangState`, `Step`).

- [ ] **Step 4: `LangState` e `Step`**

Em `language.rs`, antes dos testes:
```rust
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
```
Run: `cargo test -p gow-meet-app --lib language::` (depois do Step 6, se o crate ainda não compilar) → `6 passed`.

- [ ] **Step 5: Teste do contrato do evento `language` e da parcial**

Em `lib.rs`, no teste `events_serialize_with_kind_tag`, acrescente ao fim:
```rust
        let v = serde_json::to_value(UiEvent::Language { lang: Lang::Pt, status: LangStatus::Preparing, progress: Some(0.5) }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "language", "lang": "pt", "status": "preparing", "progress": 0.5}));
        let v = serde_json::to_value(UiEvent::Language { lang: Lang::En, status: LangStatus::Active, progress: None }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "language", "lang": "en", "status": "active"}));
        let v = serde_json::to_value(UiEvent::Partial { id: "s0".into(), speaker: Speaker::system(), t0: 0,
            text: "Olá".into(), translation: None, lang: Lang::Pt }).unwrap();
        assert_eq!(v["lang"], "pt");
```
e os testes:
```rust
    #[test]
    fn ensure_is_serialized() {
        let st = std::sync::Arc::new(std::sync::Mutex::new(()));
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hs: Vec<_> = (0..4).map(|_| {
            let (st, inside) = (st.clone(), inside.clone());
            std::thread::spawn(move || serialized(&st, || {
                assert_eq!(inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst), 0);
                std::thread::sleep(Duration::from_millis(20));
                inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }))
        }).collect();
        for h in hs { h.join().unwrap(); }
    }

    #[test]
    fn prepare_for_ended_session_is_ignored() {
        assert!(same_session(&Slot::Idle, "a").is_none());
        assert!(same_session(&Slot::Stopping, "a").is_none());
    }
```
(`use super::{…, same_session, serialized, LangStatus, UiEvent}` e `use gow_meet_core::lang::Lang;` no módulo de testes.)

Run: `cargo test -p gow-meet-app --lib`
Expected: FAIL de compilação.

- [ ] **Step 6: `lib.rs` — eventos, pares por idioma, iniciar com idioma, preparar e trocar**

Imports novos: `use gow_meet_core::lang::Lang;`, `use language::{LangState, Step};`, `use std::collections::{BTreeMap, HashMap};`.

`UiEvent`: `Partial` ganha `lang: Lang` (último campo) e entra:
```rust
    /// Idioma da reunião: `preparing` (com `progress` de 0 a 1 enquanto baixa), `active` ou `failed`.
    Language {
        lang: Lang,
        status: LangStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<f32>,
    },
```
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LangStatus { Preparing, Active, Failed }

fn emit_language(app: &AppHandle, lang: Lang, status: LangStatus, progress: Option<f32>) {
    let _ = app.emit("gow-meet", UiEvent::Language { lang, status, progress });
}
```
`TauriSink`: `PipelineEvent::Partial { id, speaker, t0, text, translation, lang, .. } => UiEvent::Partial { id, speaker, t0, text, translation, lang },`.

Estado dos modelos — troque `ModelPaths` e o campo `models` do `AppState`:
```rust
/// Modelos de um idioma já verificados (hash).
#[derive(Clone)]
struct PairPaths { whisper: PathBuf, opus_dir: PathBuf }

struct AppState {
    config: Config,
    slot: Mutex<Slot>,
    /// Pares já verificados nesta execução do app.
    pairs: Mutex<HashMap<Lang, PairPaths>>,
    /// Arquivo do modelo de voz já verificado; `None`: ainda não, ou falhou (a próxima
    /// reunião tenta de novo e, sem ele, a reunião segue sem identificar quem fala).
    speaker: Mutex<Option<PathBuf>>,
    /// Um download/verificação por vez: o preparo do outro idioma pode continuar depois
    /// que a reunião acaba e não pode escrever no mesmo `.part` que a reunião seguinte.
    ensuring: Mutex<()>,
}
```
(no `setup`: `pairs: Mutex::new(HashMap::new()), speaker: Mutex::new(None), ensuring: Mutex::new(())`.)

`Running` ganha:
```rust
    /// Identifica a sessão para o preparo do outro idioma, que roda fora do slot.
    id: String,
    lang: Arc<Mutex<LangState>>,
```

Downloads — `ensure_one` recebe o callback de progresso, e o progresso da janela principal vira uma função:
```rust
fn ensure_one(app: &AppHandle, cfg: &Config, id: &str, on_progress: &mut dyn FnMut(&str, u64, Option<u64>))
    -> Result<PathBuf, String> {
    let spec = models::manifest().remove(id).ok_or(format!("modelo {id} fora do manifesto"))?;
    let dir = local_root(app)?.join("models").join(id);
    let base = std::env::var("GOW_MEET_MODELS_URL").unwrap_or_else(|_| cfg.models_base_url.clone());
    models::ensure(&spec, &base, &dir, &HttpFetch::default(), on_progress).map_err(|e| {
        tracing::error!("baixar modelo {id}: {e:#}");
        format!("baixar modelo {id}: {e:#}")
    })?;
    Ok(dir)
}

/// Progresso para a janela principal (evento `models-progress`), ~4 por segundo por arquivo.
fn main_progress(app: &AppHandle) -> impl FnMut(&str, u64, Option<u64>) + '_ {
    let mut throttles: HashMap<String, Throttle> = HashMap::new();
    move |file, done, total| {
        let th = throttles.entry(file.to_string()).or_insert_with(Throttle::new);
        if th.allow(Instant::now(), done, total) {
            let _ = app.emit("models-progress", serde_json::json!({"file": file, "done": done, "total": total}));
        }
    }
}

/// Roda `f` com o lock de downloads.
fn serialized<T>(lock_: &Mutex<()>, f: impl FnOnce() -> T) -> T {
    let _g = lock(lock_);
    f()
}

/// Garante (baixa e confere o hash, uma vez por execução do app) os modelos de `lang`.
fn ensure_pair(app: &AppHandle, st: &AppState, lang: Lang, on_progress: &mut dyn FnMut(&str, u64, Option<u64>))
    -> Result<PairPaths, String> {
    serialized(&st.ensuring, || {
        if let Some(p) = lock(&st.pairs).get(&lang) { return Ok(p.clone()); }
        let pair = st.config.pair(lang);
        let whisper_dir = ensure_one(app, &st.config, &pair.whisper, on_progress)?;
        let file = models::manifest().remove(&pair.whisper).and_then(|s| s.files.into_iter().next())
            .ok_or(format!("modelo {} fora do manifesto", pair.whisper))?.name;
        let opus_dir = ensure_one(app, &st.config, &pair.opus, on_progress)?;
        let paths = PairPaths { whisper: whisper_dir.join(file), opus_dir };
        lock(&st.pairs).insert(lang, paths.clone());
        Ok(paths)
    })
}

/// Os arquivos de `lang` já estão no disco (sem conferir o hash): ao iniciar, o outro
/// idioma só é pré-carregado se não precisar baixar nada.
fn pair_on_disk(app: &AppHandle, cfg: &Config, lang: Lang) -> bool {
    let Ok(root) = local_root(app) else { return false };
    let pair = cfg.pair(lang);
    let manifest = models::manifest();
    [&pair.whisper, &pair.opus].into_iter().all(|id| manifest.get(id.as_str()).is_some_and(|s|
        s.files.iter().all(|f| root.join("models").join(id).join(&f.name).is_file())))
}

/// Modelo de voz: verificado uma vez por execução; falha não impede a reunião.
fn ensure_speaker(app: &AppHandle, st: &AppState) -> Option<PathBuf> {
    if let Some(p) = lock(&st.speaker).clone() { return Some(p); }
    let cfg = &st.config;
    let Some(name) = models::manifest().get(&cfg.speaker_model).and_then(|s| s.files.first()).map(|f| f.name.clone()) else {
        tracing::warn!(model = %cfg.speaker_model, "modelo de voz fora do manifesto");
        return None;
    };
    let path = serialized(&st.ensuring, || ensure_one(app, cfg, &cfg.speaker_model, &mut main_progress(app)))
        .ok().map(|dir| dir.join(name));
    lock(&st.speaker).clone_from(&path);
    path
}

/// Carrega Whisper (GPU; sem ela, processador) e Opus-MT de `lang`. Só o par inicial
/// avisa na tela da falta de GPU; o segundo par só registra no log.
fn load_pair(app: &AppHandle, cfg: &Config, lang: Lang, paths: &PairPaths, warn_gpu: bool)
    -> Result<(WhisperTranscriber, Ct2Translator), String> {
    let asr = match WhisperTranscriber::new(&paths.whisper, lang.code(), true) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(lang = lang.code(), "GPU indisponível para o Whisper: {e:#}");
            if warn_gpu {
                let _ = app.emit("gow-meet", UiEvent::Error {
                    message: "GPU indisponível: usando o processador, a tradução ficará mais lenta.".into() });
            }
            WhisperTranscriber::new(&paths.whisper, lang.code(), false).map_err(|e| format!("{e:#}"))?
        }
    };
    let tr = Ct2Translator::new(&paths.opus_dir, cfg.pair(lang).prefix).map_err(|e| format!("{e:#}"))?;
    Ok((asr, tr))
}
```
Remova `ensure_all`, `ModelPaths` e o `ensure_speaker` antigo.

Comandos e sessão:
```rust
#[tauri::command]
async fn ensure_models(app: AppHandle, lang: Lang) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_pair(&app, &st, lang, &mut main_progress(&app))?;
        ensure_speaker(&app, &st);
        Ok(())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_session(app: AppHandle, title: Option<String>, lang: Lang) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, title, lang))
        .await.map_err(|e| e.to_string())?
}
```
Em `start_blocking(app, title, lang)`: `build_session(app, &st, title, lang)`; depois de `*lock(&st.slot) = Slot::Running(Box::new(run));`:
```rust
            emit_language(app, lang, LangStatus::Active, None);
            preload_other(app, &st, lang);
```
```rust
/// Pré-carrega o outro idioma em segundo plano se ele já está no disco (troca instantânea).
fn preload_other(app: &AppHandle, st: &AppState, lang: Lang) {
    let other = lang.other();
    if !pair_on_disk(app, &st.config, other) { return; }
    let slot = lock(&st.slot);
    let Slot::Running(run) = &*slot else { return };
    if lock(&run.lang).preload(other) { spawn_prepare(app.clone(), run.id.clone(), run.lang.clone(), other); }
}
```
Em `build_session(app, st, title, lang)`:
```rust
    let cfg = &st.config;
    let paths = ensure_pair(app, st, lang, &mut main_progress(app))?;
    let speaker_model = ensure_speaker(app, st);
    let (asr, tr) = load_pair(app, cfg, lang, &paths, true)?;
    let speakers = speaker_setup(app, cfg, speaker_model.as_deref());
    let pair = cfg.pair(lang);
    let mut model_ids = vec![("asr".to_string(), pair.whisper.clone()), ("translate".to_string(), pair.opus.clone())];
```
`Meta` com `source_lang: lang.code().into(), target_lang: lang.other().code().into()`; `Pipeline::start_with_speakers(PipelineConfig { lang, ..PipelineConfig::default() }, …)`; o `Running` final ganha `id: id.clone(), lang: Arc::new(Mutex::new(LangState::new(lang)))`.

Preparo do outro idioma e troca:
```rust
/// A sessão `id` ainda está rodando: o preparo do outro idioma só entrega modelos a ela.
fn same_session<'a>(slot: &'a Slot, id: &str) -> Option<&'a Running> {
    match slot { Slot::Running(run) if run.id == id => Some(run), _ => None }
}

fn switch_now(app: &AppHandle, run: &Running, state: &mut LangState, lang: Lang) {
    run.pipeline.set_language(lang);
    state.switched(lang);
    emit_language(app, lang, LangStatus::Active, None);
}

/// Prepara o par de `lang` numa thread própria: baixa (se preciso), confere, carrega e
/// entrega ao pipeline da sessão `session`. A thread nunca guarda o pipeline: se a sessão
/// acabou (ou é outra), os modelos são descartados.
fn spawn_prepare(app: AppHandle, session: String, state: Arc<Mutex<LangState>>, lang: Lang) {
    let spawned = std::thread::Builder::new().name("prepare-lang".into()).spawn(move || {
        let t0 = Instant::now();
        let st = app.state::<AppState>();
        let mut throttle = Throttle::new();
        let mut on_progress = |_: &str, done: u64, total: Option<u64>| {
            // O pré-carregamento é silencioso: só mostra progresso se o usuário pediu este idioma.
            if lock(&state).wanted() != lang { return; }
            let Some(total) = total.filter(|t| *t > 0) else { return };
            if throttle.allow(Instant::now(), done, Some(total)) {
                emit_language(&app, lang, LangStatus::Preparing, Some(done as f32 / total as f32));
            }
        };
        let loaded = ensure_pair(&app, &st, lang, &mut on_progress)
            .and_then(|paths| load_pair(&app, &st.config, lang, &paths, false));
        let slot = lock(&st.slot);
        let Some(run) = same_session(&slot, &session) else { return };
        let mut ls = lock(&run.lang);
        match loaded {
            Ok((asr, tr)) => {
                tracing::info!(lang = lang.code(), ms = t0.elapsed().as_millis() as u64, "par de idioma pronto");
                let pair = st.config.pair(lang);
                run.pipeline.add_models(BTreeMap::from([
                    (format!("asr_{}", lang.code()), pair.whisper), (format!("translate_{}", lang.code()), pair.opus)]));
                run.pipeline.add_language(lang, Box::new(asr), Box::new(tr));
                if let Some(l) = ls.loaded(lang) { switch_now(&app, run, &mut ls, l); }
            }
            Err(e) => {
                tracing::warn!(lang = lang.code(), "par de idioma não preparado: {e}");
                if ls.failed(lang) {
                    emit_language(&app, lang, LangStatus::Failed, None);
                    emit_language(&app, ls.active(), LangStatus::Active, None);
                }
            }
        }
    });
    if let Err(e) = spawned { tracing::error!("thread de preparo do idioma não iniciou: {e}"); }
}

/// Troca o idioma falado durante a reunião (janela principal e flutuante).
#[tauri::command]
async fn set_language(app: AppHandle, lang: Lang) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let slot = lock(&st.slot);
        let Slot::Running(run) = &*slot else { return Err("nenhuma reunião em andamento".to_string()) };
        let mut ls = lock(&run.lang);
        let step = ls.request(lang);
        tracing::info!(lang = lang.code(), ?step, "pedido de troca de idioma");
        match step {
            Step::Nothing => {}
            Step::Stay(l) => emit_language(&app, l, LangStatus::Active, None),
            Step::Switch(l) => switch_now(&app, run, &mut ls, l),
            Step::Wait(l) => emit_language(&app, l, LangStatus::Preparing, None),
            Step::Prepare(l) => {
                emit_language(&app, l, LangStatus::Preparing, None);
                spawn_prepare(app.clone(), run.id.clone(), run.lang.clone(), l);
            }
        }
        Ok(())
    }).await.map_err(|e| e.to_string())?
}
```
Registre `set_language` no `generate_handler!`. Em `build.rs`, acrescente `"set_language"` à lista de comandos (e ao comentário: a flutuante pode chamar `rename_voice` e `set_language`). Em `capabilities/main.json` e `capabilities/overlay.json`, acrescente `"allow-set-language"`; na descrição do `overlay.json`: "…dar nome a uma voz e trocar o idioma durante a reunião".

Run:
```bash
cargo build -p gow-meet-app
cargo test -p gow-meet-app --lib
git status --short src-tauri/permissions
```
Expected: compila; todos os testes do app passam (os de antes + 6 de `language::`, 2 de config, `ensure_is_serialized`, `prepare_for_ended_session_is_ignored`); aparece `src-tauri/permissions/autogenerated/set_language.toml`.

- [ ] **Step 7: README**

Na tabela do `config.json`, remova as linhas de `source_lang` e `target_lang`, descreva as três chaves atuais como "(inglês)" e acrescente:
```markdown
| `whisper_model_pt` | `"whisper-small"` | Modelo de transcrição para reuniões em português |
| `opus_model_pt` | `"opus-mt-pt-en"` | Modelo de tradução português → inglês |
| `opus_prefix_pt` | nenhum | Prefixo de idioma de destino do Opus-MT pt→en |
```
(com os valores da seção **Escolha**). Acrescente uma frase: "O idioma falado é escolhido na janela principal antes de iniciar e pode ser trocado durante a reunião (janela principal ou flutuante)." Nos exemplos do CLI, mostre `--lang pt` com `--whisper …/whisper-small/ggml-small-q5_1.bin --opus …/opus-mt-pt-en`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/language.rs src-tauri/src/config.rs src-tauri/src/lib.rs src-tauri/build.rs \
  src-tauri/capabilities/main.json src-tauri/capabilities/overlay.json src-tauri/permissions README.md
git commit -m "App inicia no idioma escolhido, pré-carrega o outro e troca durante a reunião" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 5: Interface — seletor, botão na flutuante e separador

**Files:**
- Create: `src/lib/lang.ts`, `src/lib/lang.test.ts`
- Modify: `src/lib/transcript.ts`, `src/lib/transcript.test.ts`, `src/lib/status.ts`, `src/lib/status.test.ts`, `src/Overlay.tsx`, `src/Overlay.test.tsx`, `src/MainWindow.tsx`, `src/demo/Demo.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: comandos `ensure_models {lang}`, `start_session {title, lang}`, `set_language {lang}`; evento `language`; `partial.lang`; `Line.lang` (Task 4).
- Produces: nada para tarefas seguintes.

- [ ] **Step 1: Testes de `lang.ts`**

`src/lib/lang.test.ts`:
```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadLang, other, pairLabel, saveLang, switchedTo, toggleTarget } from "./lang";

function fakeStorage() {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
}

afterEach(() => vi.unstubAllGlobals());

describe("lang", () => {
  it("textos e o outro idioma", () => {
    expect(other("en")).toBe("pt");
    expect(pairLabel("pt")).toBe("PT → EN");
    expect(switchedTo("pt")).toBe("— agora em português —");
  });

  it("botão da flutuante: troca para o outro; com preparo pendente, volta ao atual", () => {
    expect(toggleTarget({ active: "en", pending: null, progress: null })).toBe("pt");
    expect(toggleTarget({ active: "en", pending: "pt", progress: 0.4 })).toBe("en");
    expect(toggleTarget({ active: null, pending: null, progress: null })).toBeNull();
  });

  it("lembra a última escolha; sem armazenamento, inglês", () => {
    vi.stubGlobal("localStorage", fakeStorage());
    expect(loadLang()).toBe("en");
    saveLang("pt");
    expect(loadLang()).toBe("pt");
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("bloqueado"); }, setItem: () => { throw new Error("bloqueado"); } });
    expect(loadLang()).toBe("en");
    expect(() => saveLang("pt")).not.toThrow();
  });
});
```
Run: `npx vitest run src/lib/lang.test.ts`
Expected: FAIL (módulo `./lang` não existe).

- [ ] **Step 2: `lang.ts`**

```ts
/** Idiomas falados (etapa 3). A tradução é sempre para o outro. */
export type Lang = "en" | "pt";
export const LANGS: Lang[] = ["en", "pt"];
export const LANG_NAME: Record<Lang, string> = { en: "Inglês", pt: "Português" };

/** Idioma da reunião: o que vale, o pedido em preparo e o progresso do download (0–1). */
export type Language = { active: Lang | null; pending: Lang | null; progress: number | null };

export const other = (l: Lang): Lang => (l === "en" ? "pt" : "en");
/** "EN → PT" */
export const pairLabel = (l: Lang) => `${l.toUpperCase()} → ${other(l).toUpperCase()}`;
/** Separador entre falas quando o idioma muda. */
export const switchedTo = (l: Lang) => `— agora em ${LANG_NAME[l].toLowerCase()} —`;
export const preparingText = (l: Lang, progress: number | null) =>
  `Preparando ${LANG_NAME[l].toLowerCase()}…${progress != null ? ` ${Math.round(progress * 100)}%` : ""}`;

/** Clique no botão da flutuante: troca para o outro idioma; com um preparo pendente, cancela
 *  (volta ao atual). `null` sem reunião. */
export function toggleTarget(l: Language): Lang | null {
  if (!l.active) return null;
  return l.pending ? l.active : other(l.active);
}

const KEY = "gow-meet-lang";

/** Última escolha da janela principal; sem armazenamento (ou bloqueado), inglês. */
export function loadLang(): Lang {
  try {
    return localStorage.getItem(KEY) === "pt" ? "pt" : "en";
  } catch {
    return "en";
  }
}

export function saveLang(l: Lang) {
  try {
    localStorage.setItem(KEY, l);
  } catch {
    // Sem armazenamento: só não lembra a escolha.
  }
}
```
Run: `npx vitest run src/lib/lang.test.ts` → `3 passed`.

- [ ] **Step 3: Testes do estado com idioma**

Em `src/lib/transcript.test.ts`, acrescente (o `import` ganha `langBreak`):
```ts
describe("idioma", () => {
  const pt = (id: string, t0: number): Line =>
    ({ id, t0, t1: t0 + 900, speaker: sys, lang: "pt", text: "Bom dia.", translation: "Good morning." });

  it("falas guardam o idioma e o separador aparece quando ele muda", () => {
    const s = run(
      { kind: "final", line: { ...line("s0", 0, "Hello team.", "Olá, equipe."), lang: "en" } },
      { kind: "partial", id: "s1", speaker: sys, t0: 1000, text: "Bom", translation: null, lang: "pt" },
      { kind: "final", line: pt("s2", 2000) },
    );
    expect([s.byId.s0.lang, s.byId.s1.lang, s.byId.s2.lang]).toEqual(["en", "pt", "pt"]);
    expect(langBreak(undefined, s.byId.s0)).toBeNull();
    expect(langBreak(s.byId.s0, s.byId.s1)).toBe("pt");
    expect(langBreak(s.byId.s1, s.byId.s2)).toBeNull();
    // Linha sem idioma (sessão antiga) não gera separador.
    expect(langBreak(run({ kind: "final", line: line("s9", 0, "x", null) }).byId.s9, s.byId.s1)).toBeNull();
  });

  it("preparing → active; o progresso fica até vir outro", () => {
    let s = run({ kind: "language", lang: "en", status: "active" });
    expect(s.language).toEqual({ active: "en", pending: null, progress: null });
    s = applyEvent(s, { kind: "language", lang: "pt", status: "preparing", progress: 0.4 });
    s = applyEvent(s, { kind: "language", lang: "pt", status: "preparing" });
    expect(s.language).toEqual({ active: "en", pending: "pt", progress: 0.4 });
    s = applyEvent(s, { kind: "language", lang: "pt", status: "active" });
    expect(s.language).toEqual({ active: "pt", pending: null, progress: null });
  });

  it("falha avisa e volta ao idioma atual; nova reunião zera", () => {
    let s = run(
      { kind: "language", lang: "en", status: "active" },
      { kind: "language", lang: "pt", status: "preparing", progress: 0.1 },
      { kind: "language", lang: "pt", status: "failed" },
      { kind: "language", lang: "en", status: "active" },
    );
    expect(s.language).toEqual({ active: "en", pending: null, progress: null });
    expect(s.notice).toBe("Não foi possível preparar o português; a reunião continua em inglês.");
    s = applyEvent(s, { kind: "reset" });
    expect(s.language).toEqual({ active: null, pending: null, progress: null });
  });
});
```
Run: `npx vitest run src/lib/transcript.test.ts`
Expected: FAIL (tipos/campos novos).

- [ ] **Step 4: `transcript.ts` com idioma**

```ts
import { LANG_NAME, type Lang, type Language } from "./lang";
```
- `Line` ganha `lang?: Lang;` (comentário: "idioma falado; ausente em sessões antigas").
- O evento `partial` ganha `lang?: Lang`; entra `| { kind: "language"; lang: Lang; status: "preparing" | "active" | "failed"; progress?: number | null }`.
- `Entry` ganha `lang: Lang | null;`.
- `State` ganha `language: Language;` e `initial` ganha `language: { active: null, pending: null, progress: null }`.
- `partial`: a entrada recebe `lang: ev.lang ?? prev?.lang ?? null`.
- `fromLine`: `lang: l.lang ?? null`.
- Novo caso:
```ts
    case "language": {
      const l = s.language;
      if (ev.status === "active") return { ...s, language: { active: ev.lang, pending: null, progress: null } };
      if (ev.status === "preparing") {
        const progress = ev.progress ?? (l.pending === ev.lang ? l.progress : null);
        return { ...s, language: { ...l, pending: ev.lang, progress } };
      }
      // failed: o backend manda em seguida o `active` do idioma atual.
      const now = l.active ? `; a reunião continua em ${LANG_NAME[l.active].toLowerCase()}` : "";
      return { ...s, language: { ...l, pending: null, progress: null },
               notice: `Não foi possível preparar o ${LANG_NAME[ev.lang].toLowerCase()}${now}.` };
    }
```
- Ao fim do arquivo:
```ts
/** Idioma da fala `e` quando ele muda em relação à fala anterior na tela (separador). */
export function langBreak(prev: Entry | undefined, e: Entry): Lang | null {
  return prev?.lang && e.lang && prev.lang !== e.lang ? e.lang : null;
}
```
Run: `npx vitest run src/lib/transcript.test.ts` → todos passam.

- [ ] **Step 5: `status.ts`: erro de ação genérico**

Em `src/lib/status.ts`, renomeie o campo `renameError` para `actionError` (comentário: "erro de ação na flutuante — dar nome ou trocar idioma —, temporário"). Em `status.test.ts`, troque `renameError` por `actionError` (inclusive no `base`) e o título do segundo teste para "erro de ação vence atraso; sem ele o atraso e o aviso voltam".

Run: `npx vitest run src/lib/status.test.ts` → passa.

- [ ] **Step 6: Testes da flutuante**

Em `src/Overlay.test.tsx`, acrescente:
```tsx
describe("OverlayView: idioma", () => {
  const fin = (id: string, t0: number, lang: "en" | "pt"): UiEvent => ({
    kind: "final", line: { id, t0, t1: t0 + 900, speaker: { source: "system", label: "Participante" }, lang,
      text: `Fictional sentence ${id}.`, translation: `Frase fictícia ${id}.` },
  });
  const setLang = () => Promise.resolve();

  it("botão mostra o par atual e o separador marca a troca", () => {
    const s = [{ kind: "language", lang: "en", status: "active" } as UiEvent, fin("s0", 0, "en"),
      { kind: "language", lang: "pt", status: "active" } as UiEvent, fin("s1", 1000, "pt")].reduce(applyEvent, initial);
    const m = renderToStaticMarkup(<OverlayView state={s} onRename={rename} onLang={setLang} />);
    expect(m).toContain("PT → EN");
    expect(m).toContain("— agora em português —");
    expect(m.indexOf("Fictional sentence s0.")).toBeLessThan(m.indexOf("— agora em português —"));
  });

  it("preparando mostra o progresso; sem reunião não há botão", () => {
    const s = [{ kind: "language", lang: "en", status: "active" } as UiEvent,
      { kind: "language", lang: "pt", status: "preparing", progress: 0.4 } as UiEvent].reduce(applyEvent, initial);
    expect(renderToStaticMarkup(<OverlayView state={s} onLang={setLang} />)).toContain("Preparando português… 40%");
    expect(renderToStaticMarkup(<OverlayView state={initial} onLang={setLang} />)).not.toContain("lang-toggle");
  });
});
```
Run: `npx vitest run src/Overlay.test.tsx`
Expected: FAIL (`onLang` não existe; sem botão).

- [ ] **Step 7: Flutuante com botão e separador**

Em `src/Overlay.tsx`:
```tsx
import { Fragment, useEffect, useState } from "react";
import { pairLabel, preparingText, switchedTo, toggleTarget, LANG_NAME, other, type Lang, type Language } from "./lib/lang";
import { langBreak, visibleEntries, type Speaker, type State } from "./lib/transcript";

type SetLang = (lang: Lang) => Promise<unknown>;

export function Overlay() {
  return (
    <OverlayView state={useGowMeet()}
      onRename={(voice, name) => invoke<string>("rename_voice", { voice, name })}
      onLang={(lang) => invoke("set_language", { lang })} />
  );
}
```
Em `OverlayView({ state, onRename, onLang, editing }: { …; onLang?: SetLang; … })`: renomeie `renameError`/`setRenameError` para `actionError`/`setActionError` (e o campo de `overlayStatus`); acrescente
```tsx
  const setLang = onLang && ((lang: Lang) => {
    setActionError(null);
    return onLang(lang).catch((e) => setActionError(`Idioma não trocado: ${String(e)}`));
  });
```
No `overlay-head`, depois do `overlay-status`:
```tsx
        <LangToggle language={state.language} onLang={setLang} />
```
Nas legendas, o `map` passa a ser:
```tsx
        {entries.map((e, i) => {
          const brk = langBreak(entries[i - 1], e);
          return (
            <Fragment key={e.id}>
              {brk && <div className="lang-break" data-tauri-drag-region>{switchedTo(brk)}</div>}
              <div className={`caption ${e.final ? "" : "partial"}`} data-tauri-drag-region>
                {/* conteúdo da legenda igual ao de antes */}
              </div>
            </Fragment>
          );
        })}
```
E o componente:
```tsx
/** "EN → PT": um clique troca para o outro idioma. Enquanto o outro prepara, mostra o
 *  progresso, e clicar de novo cancela (continua no atual). Sem reunião, nada. */
function LangToggle({ language, onLang }: { language: Language; onLang?: SetLang }) {
  const target = toggleTarget(language);
  if (!language.active || !target) return null;
  const { active, pending, progress } = language;
  return (
    <button type="button" className={`lang-toggle ${pending ? "pending" : ""}`} disabled={!onLang}
      title={pending ? `Cancelar e continuar em ${LANG_NAME[active].toLowerCase()}`
                     : `Trocar para ${LANG_NAME[other(active)].toLowerCase()}`}
      onClick={() => void onLang?.(target)}>
      {pending ? preparingText(pending, progress) : pairLabel(active)}
    </button>
  );
}
```
Run: `npx vitest run src/Overlay.test.tsx` → todos passam.

- [ ] **Step 8: Janela principal com seletor**

Em `src/MainWindow.tsx`:
```tsx
import { LANG_NAME, LANGS, loadLang, other, preparingText, saveLang, type Lang, type Language } from "./lib/lang";
```
Em `MainWindow`:
```tsx
  const [chosen, setChosen] = useState<Lang>(loadLang);
  // A troca feita na flutuante também vale como última escolha.
  const active = state.language.active;
  useEffect(() => {
    if (active) { setChosen(active); saveLang(active); }
  }, [active]);
  const onLang = (l: Lang) => {
    if (phase === "running") {
      invoke("set_language", { lang: l }).catch((e) => setError(String(e)));
      return;
    }
    setChosen(l);
    saveLang(l);
  };
```
Em `start()`: `await invoke("ensure_models", { lang: chosen });` e `await invoke("start_session", { title: title || null, lang: chosen });`. No `MainView`: `lang={phase === "running" ? active ?? chosen : chosen} language={state.language} onLang={onLang}`.

`MainViewProps` ganha:
```ts
  /** Idioma mostrado no seletor: o escolhido (parado) ou o que vale na reunião. */
  lang: Lang;
  language: Language;
  onLang: (l: Lang) => void;
```
No `MainView`, logo depois de `<div className="controls">…</div>`:
```tsx
            <LangPicker phase={phase} lang={p.lang} language={p.language} onLang={p.onLang} />
```
```tsx
/** "Idioma falado": antes de iniciar, escolhe o idioma da reunião; durante, troca. */
function LangPicker({ phase, lang, language, onLang }: {
  phase: Phase; lang: Lang; language: Language; onLang: (l: Lang) => void;
}) {
  const busy = phase === "starting" || phase === "stopping";
  const pending = phase === "running" ? language.pending : null;
  return (
    <div className="lang-picker">
      <span className="lang-label" id="lang-label">Idioma falado</span>
      <div className="lang-options" role="radiogroup" aria-labelledby="lang-label">
        {LANGS.map((l) => (
          <button key={l} type="button" role="radio" aria-checked={lang === l}
            className={`lang-option ${lang === l ? "on" : ""} ${pending === l ? "pending" : ""}`}
            disabled={busy} onClick={() => onLang(l)}>
            {LANG_NAME[l]}
          </button>
        ))}
      </div>
      <span className="lang-hint" role="status">
        {pending ? preparingText(pending, language.progress) : `tradução para ${LANG_NAME[other(lang)].toLowerCase()}`}
      </span>
    </div>
  );
}
```

- [ ] **Step 9: Estilos**

Ao fim de `src/styles.css`:
```css
/* Idioma falado: amarelo só no idioma ativo; mesma tipografia dos rótulos. */
.lang-picker { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin-top: 12px; }
.lang-label {
  font-family: var(--titulo); font-weight: 700; font-size: 12px; letter-spacing: 0.14em;
  text-transform: uppercase; color: var(--texto-2);
}
.lang-options { display: inline-flex; border: 1px solid var(--borda); border-radius: 4px; overflow: hidden; }
.lang-option {
  height: 34px; padding: 0 16px; background: transparent; border: 0; color: var(--texto-2); cursor: pointer;
  font-family: var(--titulo); font-weight: 700; font-size: 13px; letter-spacing: 0.08em; text-transform: uppercase;
}
.lang-option + .lang-option { border-left: 1px solid var(--borda); }
.lang-option.on { background: var(--amarelo); color: var(--preto); }
.lang-option.pending { color: var(--amarelo); }
.lang-option:disabled { cursor: default; opacity: 0.5; }
.lang-option:focus-visible { outline: 1px solid var(--amarelo); outline-offset: -3px; }
.lang-hint { font-size: 13px; color: var(--texto-2); }
.lang-toggle {
  flex: none; margin-left: auto; padding: 2px 8px; border-radius: 3px; background: transparent;
  border: 1px solid var(--borda); color: var(--amarelo); cursor: pointer;
  font-family: var(--titulo); font-weight: 700; font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase;
}
.overlay-status + .lang-toggle { margin-left: 0; }
.lang-toggle.pending { color: var(--texto-2); }
.lang-toggle:not(:disabled):hover { border-color: var(--amarelo); }
.lang-toggle:focus-visible { outline: 1px solid var(--amarelo); outline-offset: 2px; }
.lang-break {
  font-family: var(--titulo); font-weight: 700; font-size: 11px; letter-spacing: 0.14em;
  text-transform: uppercase; color: var(--cinza); text-align: center; margin: 2px 0;
}
```

- [ ] **Step 10: Estados do demo**

Em `src/demo/Demo.tsx`: `baseMain` ganha `lang: "en", language: { active: null, pending: null, progress: null }, onLang: noop`. Novos estados:
```tsx
  "main-idle-pt": { ...baseMain, lang: "pt", lastTranscript: "/demo/c/transcript.md" },
```
e, depois de `"main-running"`:
```tsx
  "main-running-preparing": {
    ...baseMain, phase: "running", title: "Planejamento da sprint", voices,
    lang: "en", language: { active: "en", pending: "pt", progress: 0.4 },
  },
```
Eventos e estados da flutuante:
```tsx
// Etapa 3: reunião que começou em inglês e passou para o português.
const langEv: UiEvent[] = [
  { kind: "language", lang: "en", status: "active" },
  { kind: "final", line: { id: "s0", t0: 0, t1: 2800, speaker: { source: "system", label: "Participante" }, lang: "en",
    text: "Let's review the project timeline for next week.",
    translation: "Vamos revisar o cronograma do projeto para a próxima semana." } },
  { kind: "language", lang: "pt", status: "active" },
  { kind: "final", line: { id: "s1", t0: 3200, t1: 5400, speaker: { source: "system", label: "Participante" }, lang: "pt",
    text: "O novo painel precisa ficar pronto até sexta-feira.",
    translation: "The new dashboard needs to be ready by Friday." } },
];
```
```tsx
  "overlay-languages": langEv.reduce(applyEvent, initial),
  "overlay-preparing": [...ev, { kind: "language", lang: "en", status: "active" } as UiEvent,
    { kind: "language", lang: "pt", status: "preparing", progress: 0.4 } as UiEvent].reduce(applyEvent, initial),
```
e o `OverlayView` do demo recebe `onLang={() => Promise.resolve()}`.

- [ ] **Step 11: Verificar**

```bash
npm test
npm run lint
npm run build
```
Expected: vitest todo verde (31 de antes + os novos); lint sem erros; build do TypeScript sem erros.

Confira o visual: `npx vite` e abra `http://localhost:5173/?demo=main-idle-pt`, `?demo=main-running-preparing`, `?demo=overlay-languages`, `?demo=overlay-preparing`. O seletor cabe na largura da janela principal sem quebrar a linha dos botões; o botão da flutuante fica à direita do cabeçalho.

- [ ] **Step 12: Commit**

```bash
git add src/lib/lang.ts src/lib/lang.test.ts src/lib/transcript.ts src/lib/transcript.test.ts src/lib/status.ts \
  src/lib/status.test.ts src/Overlay.tsx src/Overlay.test.tsx src/MainWindow.tsx src/demo/Demo.tsx src/styles.css
git commit -m "Seletor de idioma, botão de troca na janela flutuante e separador entre idiomas" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 6: Validação manual no Mac

Precisa do usuário (reuniões reais com consentimento dos participantes). O controlador conduz; nenhum subagente.

**Files:**
- Create: `docs/validacao-idiomas.md`

- [ ] **Step 1: Build assinado** — o usuário roda no terminal dele:
`! cd ~/Projects/gow-meet && CMAKE_POLICY_VERSION_MINIMUM=3.5 APPLE_SIGNING_IDENTITY="gow-meet dev" npx tauri build --bundles app`

- [ ] **Step 2: Reunião em português** (≥ 5 min, Teams ou Meet). Antes: escolher "Português" e iniciar (a primeira vez baixa ~190 MB + o pt→en). Durante: o usuário observa se a tradução para o inglês acompanha e é útil, e dá nome a uma voz já conhecida de reunião em inglês (deve ser reconhecida sozinha).

- [ ] **Step 3: Reunião com troca** — começar em inglês; depois de alguns minutos, trocar para português pela flutuante; mais tarde, voltar para o inglês pela janela principal.

- [ ] **Step 4: Números sem texto** — o controlador pode rodar (só ids, tempos, idiomas e contagens; nenhum texto de fala):
```bash
D="$(ls -dt ~/Library/Application\ Support/gow-meet/sessions/*/ | head -1)"
python3 - "$D" <<'PY'
import json, sys, pathlib
d = pathlib.Path(sys.argv[1])
last = {}
for raw in (d / "lines.jsonl").read_text().splitlines():
    try: l = json.loads(raw)
    except ValueError: continue
    last[l["id"]] = l
lines = sorted(last.values(), key=lambda l: l["t0"])
prev = None
for l in lines:
    lang = l.get("lang", "en")
    if lang != prev: print(f"{l['t0']/1000:7.1f}s  idioma → {lang}")
    prev = lang
from collections import Counter
print("falas por idioma:", dict(Counter(l.get("lang", "en") for l in lines)))
print("sem tradução:", sum(1 for l in lines if not l.get("translation")))
print("modelos:", json.loads((d / "meta.json").read_text()).get("models"))
PY
grep -c "idioma trocado\|par de idioma pronto" ~/Library/Application\ Support/gow-meet/logs/*.log
```
Memória e CPU com os dois pares carregados (durante a reunião, 5 amostras):
```bash
for i in 1 2 3 4 5; do ps -o rss=,%cpu= -p "$(pgrep -x gow-meet)"; sleep 10; done
```
O usuário confere, na reunião com troca, se alguma fala perto do clique saiu com transcrição num idioma e tradução no outro (ele olha a própria tela; o texto não é impresso aqui).

- [ ] **Step 5: `docs/validacao-idiomas.md`** — só números:
```markdown
# Validação manual — idiomas (etapa 3)

Máquina: Mac M2 8 GB · Build: <sha> · Data: <dd/mm/aaaa>

## Reunião em português

| Item | Resultado |
|---|---|
| Duração / falas | … min / … falas |
| Falas sem tradução | … |
| Atraso percebido (fala → tradução) | … |
| Tradução útil (avaliação do usuário) | sim/não, observação |
| Voz nomeada em reunião em inglês reconhecida em PT | sim/não |

## Reunião com troca de idioma

| Item | Resultado |
|---|---|
| Trocas (flutuante / principal) | … / … |
| Tempo do clique até a primeira fala no idioma novo | … |
| Falas com idioma misturado perto da troca | … |
| Falas perdidas na troca (percepção) | … |

## Desempenho

| Item | Resultado |
|---|---|
| Memória (RSS) com um par / com os dois | … MB / … MB |
| CPU média durante a reunião | …% |

## Pendências
```

- [ ] **Step 6: Commit**

```bash
git add docs/validacao-idiomas.md
git commit -m "Registra a validação manual das reuniões em português e da troca de idioma" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

## Marcos visíveis

- Task 1: números de qualidade e atraso do português; modelo pt→en publicado (com OK).
- Task 3: teste com os modelos reais traduz uma reunião que troca de inglês para português.
- Task 5: telas novas no demo (`?demo=main-running-preparing`, `?demo=overlay-languages`).
- Task 6: reunião real em português.
