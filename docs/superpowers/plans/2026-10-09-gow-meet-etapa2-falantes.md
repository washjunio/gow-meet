# gow-meet Etapa 2 — Quem falou — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Identificar qual participante remoto disse cada fala do áudio do sistema, permitir dar nome a cada voz durante a reunião (clicando no rótulo da janela flutuante) e lembrar das vozes nomeadas nas próximas reuniões, no mesmo computador.

**Architecture:** Três componentes novos em `gow-meet-core`: `SpeakerEmbedder` (trait; implementação `TractEmbedder` com um modelo WeSpeaker em ONNX e filterbank Kaldi em Rust), `VoiceLibrary` (`voices.json` na pasta local) e `SpeakerTracker` (biblioteca → grupos da reunião → grupo novo). O pipeline calcula a impressão de voz das falas finais do sistema na thread de transcrição, logo depois do Whisper, e a fala final já sai com `speaker.voice` e o rótulo certo. A sessão ganha `speakers.json` (rótulos por grupo) e o Markdown usa esses rótulos, inclusive para falas anteriores à renomeação. O app Tauri expõe cinco comandos e o evento `speaker`; a interface ganha o rótulo clicável na flutuante e a seção "VOZES GUARDADAS".

**Tech Stack:** Rust stable, `tract-onnx` 0.23.8 (runtime ONNX em Rust puro), `rustfft` 6.4, Tauri v2, React 18 + TypeScript + Vitest; modelo `Wespeaker/wespeaker-ecapa-tdnn512-LM` (CC BY 4.0) baixado do Hugging Face com revisão fixa e SHA-256.

**Spec:** `docs/superpowers/specs/2026-10-09-gow-meet-etapa2-falantes-design.md`

## Global Constraints

- Processamento 100% local. Nada de áudio ou impressão de voz sai da máquina.
- O áudio nunca é gravado em disco (só texto; fixtures de teste são geradas com `say` em `target/`, fora do git).
- Logs nunca contêm texto de reunião nem nomes de pessoas. Só contagens e identificadores (ex.: `voz reconhecida voice=v1 id=<id>`).
- Modelos com licença que permita uso em empresa. Licenças não comerciais (`cc-by-nc-*`) são eliminatórias. NLLB continua proibido.
- Mac primeiro, Windows em seguida.
- Identificação só no áudio do sistema. O microfone continua sempre `{"source":"mic","label":"Você"}`, sem `voice`.
- Impressões de voz existem **só** em `voices.json` e na memória durante a reunião. Nunca nos arquivos da sessão, nos logs ou na UI (`list_voices` devolve `VoiceSummary`, sem impressão).
- `voices.json` fica na pasta **local** do app (`local_root`: Mac `~/Library/Application Support/gow-meet`, Windows `%LOCALAPPDATA%\gow-meet`), como os modelos, não no perfil que sincroniza. Escrita atômica. Corrompido: aviso sem conteúdo no log, biblioteca vazia, original preservado com sufixo.
- Sessão (`<data_dir>/gow-meet/sessions/<id>/`): `lines.jsonl` continua só acrescentando; `speaker` ganha o campo opcional `voice` (`"v1"`, `"v2"`…); `speakers.json` novo, escrita atômica; `transcript.md` usa os rótulos de `speakers.json`. Sessões antigas (sem `voice` nem `speakers.json`) continuam funcionando como na etapa 1.
- Contrato com o front-end: comandos `rename_voice {voice, name}`, `list_voices`, `rename_saved_voice {id, name}`, `delete_voice {id}`, `delete_all_voices`; evento `speaker {voice, label}` para as duas janelas; `Line.speaker` ganha `voice?: string` no TypeScript.
- Comandos declarados no `AppManifest` de `src-tauri/build.rs` e permitidos só em `capabilities/main.json`; exceção: `rename_voice` também em `capabilities/overlay.json`.
- `config.json` (chaves opcionais, com padrão): `speaker_model` = `"wespeaker-ecapa512-lm"`, `speaker_library_threshold` = `0.70`, `speaker_group_threshold` = `0.65`, `speaker_min_ms` = `1000`.
- Garantias do pipeline da etapa 1 continuam: eventos de um mesmo `id` em ordem (lock de `OpenPartials`), pânico de modelo isolado com aviso genérico único, nenhuma fala no log.
- Visual da identidade GOW (`src/styles.css`): "VOCÊ" em amarelo; participantes (com ou sem nome) em cinza claro, Rajdhani caixa alta. Sem cor por pessoa.
- Texto da seção de vozes, literal: "Vozes ficam só neste computador. Apagar remove a impressão de voz; o consentimento é responsabilidade da gow."
- Consentimento (LGPD: impressão de voz é dado biométrico) é processo da gow, fora do software. O app guarda toda voz que recebe nome.
- Testes e exemplos usam só frases e nomes fictícios.
- Compilar com `export CMAKE_POLICY_VERSION_MINIMUM=3.5` (CMake 4.x, por causa do whisper.cpp e do CTranslate2).
- Toda mensagem de commit termina com uma linha em branco e exatamente:
  ```
  Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2
  ```
  (nos passos de commit isso é feito com um segundo `-m`).
- Fora do escopo: corrigir fala a fala, juntar/separar grupos, nomear depois da reunião, vozes no microfone, troca de falante dentro de uma fala, falas sobrepostas, compartilhar vozes entre computadores, nomes vindos do Teams, tradução pt→en.

## Ajustes em relação à spec (para revisão)

1. **Runtime ONNX: `tract-onnx` em vez de ONNX Runtime.** A spec diz "Roda com ONNX Runtime". Verificado neste Mac antes do plano: `ort` 2.0.0-rc.13 (ONNX Runtime 1.28, binário estático da pyke) linkado no mesmo binário que `ct2rs` gera centenas de `ld: duplicate symbol 'google::protobuf::…'` (o sentencepiece do CTranslate2 também embute protobuf). O binário linka, mas o ONNX Runtime falha com `Load model … failed: Protobuf parsing failed` e, com o tradutor carregado, o processo morre com SIGTRAP (código 133). As saídas com `ort` seriam `load-dynamic` + distribuir `libonnxruntime.dylib`/`onnxruntime.dll` junto do app (e, no Windows, mais um componente C++ no meio do conflito /MT×/MD já aberto). `tract-onnx` 0.23.8 (MIT/Apache-2.0) é Rust puro: sem C++, sem protobuf C++, sem CRT a casar no Windows. Verificado: carrega os três candidatos, dá os mesmos cossenos que o ONNX Runtime do Python (3 casas) e roda no mesmo binário que o Whisper e o CTranslate2.
2. **Evento `speaker`:** vai no canal de eventos que já existe (`"gow-meet"`, campo `kind: "speaker"`), que as duas janelas já escutam.
3. **Falas ainda abertas (parciais) do sistema:** mostram o rótulo provável (`SpeakerTracker::peek`, que não cria nem muda grupos), ou "Participante" se a voz ainda não é conhecida. A fala final é quem decide. Assim uma pessoa já reconhecida não aparece como "Participante" e depois "Ana"; uma voz nova pode ir de "Participante" para "Participante 3".
4. **`voices.json` guarda o id do modelo em cada voz (`model`).** Impressões de modelos diferentes não se comparam; se `speaker_model` mudar, as vozes antigas continuam na lista (podem ser apagadas), mas não são reconhecidas.
5. **Vozes guardadas só mudam fora de reunião.** Durante a reunião o rastreador tem a biblioteca em memória e a gravaria de novo ao encerrar (uma voz apagada voltaria). A janela principal desabilita Renomear/Apagar durante a reunião e o backend recusa com "Encerre a reunião para alterar as vozes guardadas.".
6. **"Apagar todas" também apaga as cópias preservadas de arquivos corrompidos** (`voices.json.corrompido-*`) e o temporário: elas também contêm impressões de voz. "Apagar" de uma voz não mexe nessas cópias.
7. **Fala curta sem falante anterior:** sai como "Participante" genérico, sem `voice` (não dá para nomear).
8. **Duração da fala** para a regra de 1 s: duração da fala final menos o silêncio de fechamento do segmentador (600 ms), que também fica fora da impressão.
9. **"Última vez vista"** é o campo `updated`, atualizado quando a voz é reconhecida ou nomeada numa reunião (renomear na lista não muda).
10. **Nomes:** aparados, de 1 a 60 caracteres. Nome igual (sem diferenciar maiúsculas) a uma voz guardada do mesmo modelo soma a essa voz.
11. **Grupo nomeado pelo usuário e depois renomeado** (ex.: "Ana" por engano, depois "Bruno"): a voz "Ana" fica com as falas somadas na primeira vez; a spec só protege o caso do grupo *reconhecido* como outra voz, que está coberto.

## Review Focus

1. Renomear um grupo enquanto a tradução de uma fala dele ainda não chegou → a tradução chega com o rótulo antigo dentro do `Line`; a tela deve continuar mostrando o nome novo (teste na Task 7: "tradução que chega depois do nome mantém o nome").
2. Apagar uma voz guardada com uma reunião em andamento → a voz voltaria quando a reunião encerra; o app recusa a alteração e a interface desabilita os botões (teste na Task 6: `saved_voices_only_change_outside_a_meeting`).
3. "Apagar todas" depois de um `voices.json` corrompido → a cópia preservada também tem impressões de voz e precisa sumir (teste na Task 3: `rename_delete_and_delete_all`).
4. Trocar `speaker_model` no `config.json` → vozes guardadas de outro modelo nunca podem ser comparadas (vetores de outro espaço/tamanho) nem causar pânico (teste na Task 3: `best_match_only_compares_same_model` e `same_name_adds_to_existing_voice`).
5. Abrir ou recuperar uma sessão gravada pela etapa 1 (sem `voice`, sem `speakers.json`) → tudo continua igual (teste na Task 4: `old_sessions_without_voice_or_speakers_json_still_work`).

## Estrutura de arquivos

```text
crates/core/
  Cargo.toml                      + tract-onnx, rustfft; dev: tracing-subscriber
  models.json                     + "wespeaker-ecapa512-lm"
  src/lib.rs                      + pub mod fbank, speaker, tracker, voices
  src/fbank.rs                    NOVO: filterbank log-mel estilo Kaldi (entrada do WeSpeaker) + CMN
  src/speaker.rs                  NOVO: trait SpeakerEmbedder, TractEmbedder, normalize, cosine, DEFAULT_SPEAKER_MODEL
  src/voices.rs                   NOVO: VoiceLibrary (voices.json), Voice, VoiceSummary, clean_name
  src/tracker.rs                  NOVO: SpeakerTracker, TrackerConfig, Assignment
  src/session.rs                  Speaker.voice, SpeakerEntry/Speakers, speakers.json, Markdown e recuperação com nomes
  src/pipeline.rs                 SpeakerId, start_with_speakers, rename_voice, PipelineEvent::Speaker
  tests/speaker_say.rs            NOVO (ignorado): vozes do `say` → impressões, rastreador, pipeline
  tests/voice_privacy.rs          NOVO: impressões e nomes fora dos logs e dos arquivos da sessão
crates/cli/src/main.rs            trata PipelineEvent::Speaker
scripts/gen-voice-fixtures.sh     NOVO: falas fictícias com 4 vozes do `say` em target/voice-fixtures
scripts/measure-voice-models.py   NOVO: compara os candidatos (licença já filtrada; tamanho, tempo, qualidade)
src-tauri/
  Cargo.toml                      + anyhow
  build.rs                        + 5 comandos no AppManifest
  capabilities/main.json          + allow-* dos 5 comandos
  capabilities/overlay.json       + allow-rename-voice
  permissions/autogenerated/*.toml  gerados pelo build (commitados, como os da etapa 1)
  src/config.rs                   + speaker_model, limites, duração mínima; Config::tracker()
  src/lib.rs                      UiEvent::Speaker, modelo de voz opcional, speaker_setup, 5 comandos
src/
  lib/transcript.ts               Speaker.voice, evento speaker, State.labels
  lib/rename.ts                   NOVO: teclas do campo de nome (Enter/Esc/vazio)
  Overlay.tsx                     rótulo clicável + campo inline
  SavedVoices.tsx                 NOVO: seção "VOZES GUARDADAS"
  MainWindow.tsx                  liga a seção aos comandos
  icons.tsx                       + IconPencil
  styles.css                      rótulo editável, campo, lista de vozes
  demo/Demo.tsx                   estados novos (flutuante com nomes/campo aberto; vozes, renomear, confirmar, erro)
docs/modelo-de-voz.md             NOVO: medição e escolha do modelo (só números)
docs/validacao-falantes.md        NOVO: validação manual (só números)
README.md                         modelo e licença, config.json, onde ficam as vozes, privacidade
```

## Evidências levantadas antes do plano

Medido neste Mac (M2) em 2026-10-09, com 4 vozes do `say` (Samantha, Daniel, Karen, Moira) × 8 frases fictícias. A Task 1 refaz a medição e confirma.

| Candidato (Hugging Face, revisão) | Licença | Arquivo | Dim. | Entrada | tract p90/fala | Margem intra−inter | Agrupamento 0,65 |
|---|---|---|---|---|---|---|---|
| `Wespeaker/wespeaker-ecapa-tdnn512-LM` @ `a2f3dcb1c8702caccc7a55ceb57f5e8d1842112b` | CC BY 4.0 | `voxceleb_ECAPA512_LM.onnx`, 24,9 MB | 192 | `feats` [B, T, 80] fbank | 38 ms | +0,058 | 4 grupos, 100% puros |
| `Wespeaker/wespeaker-voxceleb-campplus-LM` @ `c5e01c6fcffcce160861e7e79782828320192b5c` | Apache-2.0 | `voxceleb_CAM++_LM.onnx`, 29,3 MB | 512 | `feats` [B, T, 80] fbank | 76 ms | +0,061 | 4 grupos, 100% puros |
| `Wespeaker/wespeaker-voxceleb-resnet34-LM` @ `f0c48c298fd835726c27956a5d617bad7115627e` | CC BY 4.0 | `voxceleb_resnet34_LM.onnx`, 26,5 MB | 256 | `feats` [B, T, 80] fbank | 206 ms | +0,040 | 4 grupos, 100% puros |

- Os três recebem a mesma entrada: filterbank Kaldi de 80 filtros (25 ms / 10 ms, janela Hamming, amostras em escala int16, sem dither) com subtração da média por filtro (CMN), como o `wespeaker/cli/speaker.py` faz na inferência. Opset 14, IR 7. O código em Rust da Task 2 bate com o `kaldi-native-fbank` 1.20.0 (diferença < 0,02 por filtro) e as impressões batem com o ONNX Runtime do Python.
- Com limite de grupo 0,60 os três juntam Samantha e Karen num grupo só; com 0,65 e 0,70 separam as quatro vozes. Daí os padrões 0,65 (grupo) e 0,70 (biblioteca, mais exigente: errar um nome é pior que criar um grupo).
- Rejeitados: `pyannote/wespeaker-voxceleb-resnet34-LM` (formato pyannote/PyTorch, sem ONNX), `Revai/pyannote-wespeaker-voxceleb-resnet34-LM` (`cc-by-nc-4.0`, não comercial), `nvidia/speakerverification_en_titanet_large` (`.nemo`, exigiria exportar para ONNX), `onnx-community/wespeaker-voxceleb-resnet34-LM` (reexportação de terceiros do mesmo ResNet34, que já é o mais lento).
- `ort` 2.0.0-rc.13 com `ct2rs` no mesmo binário: protobuf duplicado e falha ao carregar o modelo (ver Ajuste 1). `tract-onnx` 0.23.8 no mesmo binário que `whisper-rs` e `ct2rs`: funciona.

---

### Task 1: Escolha do modelo de voz, manifesto e dependência no CI

**Files:**
- Create: `scripts/gen-voice-fixtures.sh`, `scripts/measure-voice-models.py`, `docs/modelo-de-voz.md`
- Modify: `crates/core/models.json`, `crates/core/src/models.rs` (teste `manifest_has_expected_ids_and_real_hashes`), `crates/core/Cargo.toml`, `README.md`

**Interfaces:**
- Consumes: `models::manifest()` e `ModelSpec`/`ModelFile` (etapa 1, `crates/core/src/models.rs`).
- Produces:
  - entrada `"wespeaker-ecapa512-lm"` em `crates/core/models.json`, um arquivo: `name` = `"voxceleb_ECAPA512_LM.onnx"`. É o id usado por `speaker::DEFAULT_SPEAKER_MODEL` (Task 2) e pelo padrão `speaker_model` do `config.json` (Task 6).
  - fixtures fora do git: `target/voice-fixtures/<Voz>_<i>.wav`, 16 kHz mono, `<Voz>` sem espaços, `i` = 0…7 (frases 0–3 = "reunião 1", 4–7 = "reunião 2"). Variável `VOICES` muda as vozes.
  - dependências em `gow-meet-core`: `tract-onnx = "=0.23.8"`, `rustfft = "6.4"`.

- [ ] **Step 1: Conferir as vozes do `say`**

Run: `say -v '?' | grep -E '^(Samantha|Daniel|Karen|Moira) '`
Expected: uma linha (ou mais) para cada um dos 4 nomes, todas `en_*`. Se faltar alguma, escolha outra voz inglesa de **uma palavra só** na lista de `say -v '?' | grep en_` (ex.: `Rishi`, `Tessa`, `Fred`, `Kathy`) e use `VOICES="Samantha Daniel Rishi Moira"` nos comandos abaixo. Prefira vozes de sotaques diferentes, mas mantenha um par parecido (Samantha e Karen são parecidas: é o que testa o limite de grupo).

- [ ] **Step 2: Script que gera as falas fictícias**

`scripts/gen-voice-fixtures.sh`:
```bash
#!/usr/bin/env bash
# Gera falas fictícias em inglês com vozes diferentes do macOS, para o teste de
# identificação de falantes. Saída fora do git (target/). Só frases inventadas.
# Confira as vozes instaladas com: say -v '?' | grep en_
set -euo pipefail
OUT=${1:-target/voice-fixtures}
VOICES=${VOICES:-"Samantha Daniel Karen Moira"}
mkdir -p "$OUT"
SENTENCES=(
  "Let's review the project timeline for next week."
  "The new dashboard should be ready by Friday."
  "Can you share the budget estimate with the team?"
  "I will send a summary after this call."
  "We should move the review meeting to Thursday afternoon."
  "The client asked for a shorter onboarding flow."
  "Please update the shared spreadsheet before lunch."
  "I think the second option is cheaper in the long run."
)
for v in $VOICES; do
  for i in "${!SENTENCES[@]}"; do
    say -v "$v" --data-format=LEI16@16000 -o "$OUT/${v}_$i.wav" "${SENTENCES[$i]}"
  done
done
ls "$OUT" | wc -l
```

Run: `chmod +x scripts/gen-voice-fixtures.sh && bash scripts/gen-voice-fixtures.sh`
Expected: `32` (4 vozes × 8 frases) em `target/voice-fixtures/`.

- [ ] **Step 3: Conferir licença, revisão e hashes no Hugging Face**

Run:
```bash
for id in Wespeaker/wespeaker-ecapa-tdnn512-LM Wespeaker/wespeaker-voxceleb-campplus-LM Wespeaker/wespeaker-voxceleb-resnet34-LM; do
  curl -s "https://huggingface.co/api/models/$id?blobs=true" | python3 -c '
import sys, json
m = json.load(sys.stdin)
print(m["id"], m["sha"], [t for t in m["tags"] if t.startswith("license:")])
for s in m["siblings"]:
    if s["rfilename"].endswith(".onnx"): print("  ", s["rfilename"], s["size"], s["lfs"]["sha256"])'
done
```
Expected (o `sha` pode ter avançado se o repositório mudou; o plano fixa as revisões abaixo):
```text
Wespeaker/wespeaker-ecapa-tdnn512-LM a2f3dcb1c8702caccc7a55ceb57f5e8d1842112b ['license:cc-by-4.0']
   voxceleb_ECAPA512_LM.onnx 24861931 d71b85d9b48058ef68004f04f1b78acebefb9dfcf542e19b976a12a5ad1f10b0
Wespeaker/wespeaker-voxceleb-campplus-LM c5e01c6fcffcce160861e7e79782828320192b5c ['license:apache-2.0']
   voxceleb_CAM++_LM.onnx 29292449 1068e4ac3a76bb9c769e6816ef30bf89363f6e966f1d938210cb8ed4038f8e93
Wespeaker/wespeaker-voxceleb-resnet34-LM f0c48c298fd835726c27956a5d617bad7115627e ['license:cc-by-4.0']
   voxceleb_resnet34_LM.onnx 26530309 7bb2f06e9df17cdf1ef14ee8a15ab08ed28e8d0ef5054ee135741560df2ec068
```
CC BY 4.0 e Apache-2.0 permitem uso em empresa (CC BY exige atribuição: Step 9). Qualquer `license:cc-by-nc-*` elimina o candidato.

- [ ] **Step 4: Baixar os candidatos na revisão fixa e conferir o SHA-256 local**

Run:
```bash
mkdir -p target/voice-models && cd target/voice-models
curl -fL -o voxceleb_ECAPA512_LM.onnx https://huggingface.co/Wespeaker/wespeaker-ecapa-tdnn512-LM/resolve/a2f3dcb1c8702caccc7a55ceb57f5e8d1842112b/voxceleb_ECAPA512_LM.onnx
curl -fL -o voxceleb_CAM++_LM.onnx "https://huggingface.co/Wespeaker/wespeaker-voxceleb-campplus-LM/resolve/c5e01c6fcffcce160861e7e79782828320192b5c/voxceleb_CAM%2B%2B_LM.onnx"
curl -fL -o voxceleb_resnet34_LM.onnx https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM/resolve/f0c48c298fd835726c27956a5d617bad7115627e/voxceleb_resnet34_LM.onnx
shasum -a 256 *.onnx
cd -
```
Expected:
```text
1068e4ac3a76bb9c769e6816ef30bf89363f6e966f1d938210cb8ed4038f8e93  voxceleb_CAM++_LM.onnx
d71b85d9b48058ef68004f04f1b78acebefb9dfcf542e19b976a12a5ad1f10b0  voxceleb_ECAPA512_LM.onnx
7bb2f06e9df17cdf1ef14ee8a15ab08ed28e8d0ef5054ee135741560df2ec068  voxceleb_resnet34_LM.onnx
```
Os hashes locais têm de ser iguais aos do Step 3. São eles que vão para o manifesto.

- [ ] **Step 5: Script de medição**

`scripts/measure-voice-models.py`:
```python
"""Compara modelos de impressão de voz com as falas fictícias do `say`.

Uso (só para a escolha do modelo; nada disto vai para o app):
  uv run --no-project --with onnxruntime==1.20.1 --with kaldi-native-fbank==1.20.0 --with numpy \
    python -I scripts/measure-voice-models.py target/voice-models/*.onnx target/voice-fixtures

Para cada modelo: dimensão, tempo por fala (mediana/p90, CPU, 1 thread), semelhança
entre falas da mesma voz (intra) e de vozes diferentes (inter), margem = intra mín − inter
máx, e o agrupamento em tempo real (mesma regra do SpeakerTracker) com limites 0,60/0,65/0,70.
"""
import glob, itertools, os, sys, time, wave

import kaldi_native_fbank as knf
import numpy as np
import onnxruntime as ort


def read(path):
    with wave.open(path) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1, path
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32)


def fbank(pcm_i16):
    # Igual ao WeSpeaker na inferência: escala int16, sem dither, janela hamming, 80 filtros, CMN.
    o = knf.FbankOptions()
    o.frame_opts.dither = 0.0
    o.frame_opts.window_type = "hamming"
    o.mel_opts.num_bins = 80
    f = knf.OnlineFbank(o)
    f.accept_waveform(16000, pcm_i16.tolist())
    f.input_finished()
    m = np.stack([f.get_frame(i) for i in range(f.num_frames_ready)])
    return (m - m.mean(axis=0)).astype(np.float32)


def cluster(order, embs, thr):
    groups = []  # [soma, {voz: falas}]
    for p in order:
        e, who = embs[p], os.path.basename(p).rsplit("_", 1)[0]
        sims = [g[0] @ e / np.linalg.norm(g[0]) for g in groups]
        if sims and max(sims) >= thr:
            g = groups[int(np.argmax(sims))]
            g[0] += e
            g[1][who] = g[1].get(who, 0) + 1
        else:
            groups.append([e.copy(), {who: 1}])
    pure = sum(max(g[1].values()) for g in groups)
    return len(groups), pure / len(order)


models, wav_dir = sys.argv[1:-1], sys.argv[-1]
wavs = sorted(glob.glob(os.path.join(wav_dir, "*.wav")))
voices = sorted({os.path.basename(p).rsplit("_", 1)[0] for p in wavs})
n = max(int(os.path.basename(p).rsplit("_", 1)[1][:-4]) for p in wavs) + 1
order = [os.path.join(wav_dir, f"{v}_{i}.wav") for i in range(n) for v in voices]
feats = {p: fbank(read(p)) for p in wavs}
print(f"{len(voices)} vozes, {len(wavs)} falas")
for mp in models:
    so = ort.SessionOptions()
    so.intra_op_num_threads = 1
    s = ort.InferenceSession(mp, so, providers=["CPUExecutionProvider"])
    embs, times = {}, []
    for p, f in feats.items():
        t = time.perf_counter()
        e = s.run(None, {"feats": f[None]})[0][0]
        times.append((time.perf_counter() - t) * 1000)
        embs[p] = e / np.linalg.norm(e)
    spk = lambda p: os.path.basename(p).rsplit("_", 1)[0]
    intra, inter = [], []
    for a, b in itertools.combinations(wavs, 2):
        (intra if spk(a) == spk(b) else inter).append(float(embs[a] @ embs[b]))
    groups = " ".join(f"{thr:.2f}→{g} grupos/{pur:.0%} puro" for thr in (0.60, 0.65, 0.70)
                      for g, pur in [cluster(order, embs, thr)])
    print(f"{os.path.basename(mp)} ({os.path.getsize(mp) / 1e6:.1f} MB): dim={len(embs[wavs[0]])} | "
          f"ms/fala mediana={np.median(times):.0f} p90={np.percentile(times, 90):.0f} | "
          f"intra mín={min(intra):.3f} | inter máx={max(inter):.3f} | margem={min(intra) - max(inter):+.3f} | {groups}")
```

- [ ] **Step 6: Medir**

Precisa do `uv` (`brew install uv` se `uv --version` falhar). O ONNX Runtime do Python aqui é só régua de medição; o app usa o `tract` (o tempo no app é medido em Rust na Task 2).

Run:
```bash
uv run --no-project --with onnxruntime==1.20.1 --with kaldi-native-fbank==1.20.0 --with numpy \
  python -I scripts/measure-voice-models.py target/voice-models/*.onnx target/voice-fixtures
```
Expected (números do levantamento; os seus podem variar um pouco):
```text
4 vozes, 32 falas
voxceleb_CAM++_LM.onnx (29.3 MB): dim=512 | ms/fala mediana=42 p90=45 | intra mín=0.705 | inter máx=0.644 | margem=+0.061 | 0.60→3 grupos/75% puro 0.65→4 grupos/100% puro 0.70→4 grupos/100% puro
voxceleb_ECAPA512_LM.onnx (24.9 MB): dim=192 | ms/fala mediana=32 p90=35 | intra mín=0.677 | inter máx=0.618 | margem=+0.058 | 0.60→3 grupos/75% puro 0.65→4 grupos/100% puro 0.70→4 grupos/100% puro
voxceleb_resnet34_LM.onnx (26.5 MB): dim=256 | ms/fala mediana=146 p90=155 | intra mín=0.694 | inter máx=0.654 | margem=+0.040 | 0.60→3 grupos/75% puro 0.65→4 grupos/100% puro 0.70→4 grupos/100% puro
```

- [ ] **Step 7: Decidir e registrar**

Critérios, em ordem:
1. Licença permite uso em empresa (eliminatório).
2. Tamanho ≤ 40 MB (eliminatório: vai junto dos ~700 MB de modelos, mas não deve dobrar o download).
3. Qualidade: 4 grupos 100% puros com limite 0,65 (eliminatório). Entre os aprovados, maior margem; diferença < 0,01 é empate.
4. Desempate: menor p90 por fala; depois, menor arquivo.
5. Confirmação em Rust na Task 2: p90 ≤ 150 ms por fala com o `tract` no M2 (no levantamento: ECAPA 38 ms, CAM++ 76 ms, ResNet34 206 ms — o ResNet34 reprova).

Com os números do levantamento: os três passam em 1–3; ECAPA e CAM++ empatam em qualidade (+0,058 × +0,061); o ECAPA ganha no tempo (metade) e no tamanho. **Escolha: `Wespeaker/wespeaker-ecapa-tdnn512-LM`**, id `wespeaker-ecapa512-lm`. Se a sua medição inverter o resultado (CAM++ com margem ≥ 0,01 maior), use o CAM++: entrada do manifesto abaixo, id `wespeaker-campplus-lm`, e o mesmo id em `DEFAULT_SPEAKER_MODEL` (Task 2) e no teste `speaker_defaults_and_override` (Task 6). O código é o mesmo para os dois (mesma entrada).

`docs/modelo-de-voz.md` (cole os números da sua execução no lugar dos do levantamento):
```markdown
# Modelo de impressão de voz — escolha (etapa 2)

Data: 2026-10-09 · Máquina: Mac M2 8 GB · Falas: 4 vozes do `say` × 8 frases fictícias (`scripts/gen-voice-fixtures.sh`)

| Modelo (revisão) | Licença | MB | Dim. | p90 ONNX Runtime/Python | p90 tract/Rust (Task 2) | Margem | Grupos a 0,60 / 0,65 / 0,70 |
|---|---|---|---|---|---|---|---|
| wespeaker-ecapa-tdnn512-LM (a2f3dcb) | CC BY 4.0 | 24,9 | 192 | 35 ms | 38 ms | +0,058 | 3 / 4 / 4 |
| wespeaker-voxceleb-campplus-LM (c5e01c6) | Apache-2.0 | 29,3 | 512 | 45 ms | 76 ms | +0,061 | 3 / 4 / 4 |
| wespeaker-voxceleb-resnet34-LM (f0c48c2) | CC BY 4.0 | 26,5 | 256 | 155 ms | 206 ms | +0,040 | 3 / 4 / 4 |

Escolha: wespeaker-ecapa-tdnn512-LM (empate em qualidade com o CAM++, metade do tempo, menor).
Limites padrão: grupo 0,65 (0,60 junta duas vozes parecidas), biblioteca 0,70.
Runtime: tract-onnx 0.23.8 (o ONNX Runtime estático colide com o protobuf do CTranslate2).
```

- [ ] **Step 8: Teste do manifesto (falhando)**

Em `crates/core/src/models.rs`, no teste `manifest_has_expected_ids_and_real_hashes`, troque a linha do `for`:
```rust
        for id in ["whisper-small-en", "whisper-turbo", "opus-mt-en-pt", "wespeaker-ecapa512-lm"] {
```

Run: `cargo test -p gow-meet-core --lib manifest_has_expected_ids`
Expected: FAIL com `falta wespeaker-ecapa512-lm`.

- [ ] **Step 9: Entrada no manifesto, dependências e atribuição**

`crates/core/models.json`: depois da entrada `"opus-mt-en-pt"` (não esqueça a vírgula depois do `}` dela), acrescente:
```json
  "wespeaker-ecapa512-lm": {
    "files": [
      {
        "name": "voxceleb_ECAPA512_LM.onnx",
        "path": "Wespeaker/wespeaker-ecapa-tdnn512-LM/resolve/a2f3dcb1c8702caccc7a55ceb57f5e8d1842112b/voxceleb_ECAPA512_LM.onnx",
        "sha256": "d71b85d9b48058ef68004f04f1b78acebefb9dfcf542e19b976a12a5ad1f10b0"
      }
    ]
  }
```
(Se a escolha for o CAM++: id `"wespeaker-campplus-lm"`, `"name": "voxceleb_CAM++_LM.onnx"`, `"path": "Wespeaker/wespeaker-voxceleb-campplus-LM/resolve/c5e01c6fcffcce160861e7e79782828320192b5c/voxceleb_CAM%2B%2B_LM.onnx"` — o `+` vai codificado na URL —, `"sha256": "1068e4ac3a76bb9c769e6816ef30bf89363f6e966f1d938210cb8ed4038f8e93"`.)

`crates/core/Cargo.toml`, em `[dependencies]`, depois da linha do `reqwest`:
```toml
# Impressão de voz: inferência ONNX em Rust puro (sem C++: não colide com o protobuf do
# sentencepiece/ct2rs nem com o CRT do Windows) e FFT para o filterbank.
tract-onnx = "=0.23.8"
rustfft = "6.4"
```

`README.md`, no fim do arquivo (depois do parágrafo sobre `GOW_MEET_MODELS_URL`), acrescente:
```markdown
## Modelos e licenças

| Modelo | Uso | Licença |
| --- | --- | --- |
| whisper.cpp `small.en` / `large-v3-turbo` | transcrição | MIT |
| Opus-MT en→pt (convertido para CTranslate2) | tradução | CC BY 4.0 |
| WeSpeaker ECAPA-TDNN512-LM (`Wespeaker/wespeaker-ecapa-tdnn512-LM`, treinado no VoxCeleb2) | impressão de voz (quem falou) | CC BY 4.0 — © projeto WeSpeaker (wenet-e2e) |
```

Run:
```bash
export CMAKE_POLICY_VERSION_MINIMUM=3.5
cargo test -p gow-meet-core --lib manifest_has_expected_ids
cargo build -p gow-meet-core
```
Expected: `test result: ok. 1 passed`; build sem erros (o `tract-onnx` compila só Rust, sem CMake).

- [ ] **Step 10: Commit**

```bash
git add scripts/gen-voice-fixtures.sh scripts/measure-voice-models.py docs/modelo-de-voz.md \
  crates/core/models.json crates/core/src/models.rs crates/core/Cargo.toml Cargo.lock README.md
git commit -m "Escolhe o modelo de impressão de voz e adiciona ao manifesto" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

- [ ] **Step 11: Observar a dependência nova no CI do Windows**

O job do Windows segue com `continue-on-error` porque o `ct2rs` não linka (LNK2038 /MT × /MD); isso **não** é para corrigir aqui. O objetivo é ver que o `tract-onnx`/`rustfft` compilam no MSVC e não acrescentam erro de link novo.

Peça autorização ao usuário para enviar a branch. Depois:
```bash
git push -u origin etapa2
RUN=$(gh run list --branch etapa2 --limit 1 --json databaseId -q '.[0].databaseId')
gh run watch "$RUN" --exit-status || true
JOB=$(gh run view "$RUN" --json jobs -q '.jobs[] | select(.name | test("windows")) | .databaseId')
gh run view "$RUN" --log --job "$JOB" | grep -E "Compiling (tract|rustfft)|LNK[0-9]+|error(\[|:)" | head -40
```
Expected: no job do Windows aparecem `Compiling tract-onnx v0.23.8` e `Compiling rustfft v6.4.x` sem erro; os únicos `LNK2038`/`LNK2005` citam objetos do CTranslate2/sentencepiece (como antes). Anote o resultado em `docs/modelo-de-voz.md` numa linha `CI Windows (commit <sha>): tract e rustfft compilam; link segue falhando só pelo ct2rs (LNK2038)` — ou descreva o erro novo, se houver — e faça um commit pequeno:
```bash
git add docs/modelo-de-voz.md
git commit -m "Registra o efeito do tract no CI do Windows" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 2: Filterbank, `SpeakerEmbedder` com tract e teste com vozes do `say`

**Files:**
- Create: `crates/core/src/fbank.rs`, `crates/core/src/speaker.rs`, `crates/core/tests/speaker_say.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: dependências `tract-onnx`/`rustfft` e o id `"wespeaker-ecapa512-lm"` (Task 1); `wav::read_wav_16k_mono(path: &Path) -> Result<Vec<f32>>` (etapa 1); fixtures `target/voice-fixtures/<Voz>_<i>.wav` (Task 1).
- Produces:
  - `fbank::NUM_BINS: usize` (= 80); `fbank::Fbank::new() -> Fbank`; `Fbank::compute(&self, audio: &[f32]) -> Vec<[f32; NUM_BINS]>` (16 kHz mono em [-1, 1]); `fbank::cmn(feats: &mut [[f32; NUM_BINS]])`.
  - `speaker::SpeakerEmbedder` — `pub trait SpeakerEmbedder: Send { fn embed(&mut self, audio: &[f32]) -> anyhow::Result<Vec<f32>>; }` (16 kHz mono; devolve vetor com norma 1).
  - `speaker::TractEmbedder::new(model_path: &Path) -> anyhow::Result<TractEmbedder>` (implementa `SpeakerEmbedder`; `Send`).
  - `speaker::normalize(v: &mut [f32]) -> bool`; `speaker::cosine(a: &[f32], b: &[f32]) -> f32` (−1 para tamanhos diferentes ou vetor nulo).
  - `speaker::DEFAULT_SPEAKER_MODEL: &str` = `"wespeaker-ecapa512-lm"`; `speaker::MIN_EMBED_SAMPLES: usize` = 8 000.
  - `crates/core/tests/speaker_say.rs` com `fn fixtures() -> BTreeMap<(String, usize), Vec<f32>>` (lê `GOW_MEET_VOICES`), que as Tasks 3 e 5 estendem.

- [ ] **Step 1: Testes do filterbank (falhando)**

A referência foi calculada com `kaldi-native-fbank` 1.20.0 (`dither=0`, `window_type="hamming"`, 80 filtros) sobre o mesmo sinal, em escala int16 — a entrada que o WeSpeaker usa.

`crates/core/src/fbank.rs` (por enquanto só os testes):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Referência: kaldi-native-fbank 1.20.0 (dither 0, janela hamming, 80 filtros) sobre
    /// o mesmo sinal, em escala int16.
    #[test]
    fn matches_kaldi_native_fbank() {
        let x: Vec<f32> = (0..16_000).map(|n| {
            let t = n as f32 / 16_000.0;
            (8_000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                + 3_000.0 * (2.0 * std::f32::consts::PI * 1_234.0 * t).sin()) / 32_768.0
        }).collect();
        let f = Fbank::new().compute(&x);
        assert_eq!(f.len(), 98);
        for (t, b, want) in [(0, 0, 11.671196), (0, 10, 11.286946), (0, 40, 13.254454),
                             (50, 5, 11.499412), (50, 79, 11.802671), (97, 20, 12.014282)] {
            assert!((f[t][b] - want).abs() < 0.02, "f[{t}][{b}] = {} (esperado {want})", f[t][b]);
        }
        let s: f32 = f[0].iter().sum();
        assert!((s - 1084.5693).abs() < 0.5, "soma = {s}");
    }

    #[test]
    fn short_audio_gives_no_frames_and_cmn_zeroes_mean() {
        assert!(Fbank::new().compute(&[0.1; 399]).is_empty());
        let mut f = vec![[1.0; NUM_BINS], [3.0; NUM_BINS]];
        cmn(&mut f);
        assert_eq!(f[0][0], -1.0);
        assert_eq!(f[1][79], 1.0);
    }
}
```

`crates/core/src/lib.rs`: acrescente `pub mod fbank;` depois de `pub mod capture;`.

Run: `cargo test -p gow-meet-core --lib fbank::`
Expected: erro de compilação (`cannot find type Fbank`, `cannot find value NUM_BINS`).

- [ ] **Step 2: Implementar o filterbank**

No topo de `crates/core/src/fbank.rs`, antes dos testes:
```rust
//! Filterbank log-mel no formato Kaldi, como o WeSpeaker usa na inferência:
//! amostras em escala int16, janela de 25 ms a cada 10 ms (snip_edges), DC removido,
//! pré-ênfase 0,97, janela de Hamming, FFT de 512, potência, 80 filtros mel de 20 Hz a
//! 8 kHz, log com piso em f32::EPSILON. `cmn` subtrai a média de cada filtro no tempo.
use rustfft::{num_complex::Complex32, FftPlanner};
use std::sync::Arc;

pub const NUM_BINS: usize = 80;
const FRAME_LEN: usize = 400;
const FRAME_SHIFT: usize = 160;
const FFT_LEN: usize = 512;
const PREEMPH: f32 = 0.97;
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 8_000.0;

fn mel(hz: f32) -> f32 { 1127.0 * (1.0 + hz / 700.0).ln() }

pub struct Fbank {
    window: Vec<f32>,
    /// Por filtro: primeiro índice da FFT e pesos.
    filters: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl Default for Fbank {
    fn default() -> Self { Self::new() }
}

impl Fbank {
    pub fn new() -> Self {
        let window = (0..FRAME_LEN)
            .map(|i| 0.54 - 0.46 * (2.0 * std::f32::consts::PI * i as f32 / (FRAME_LEN - 1) as f32).cos())
            .collect();
        let (lo, hi) = (mel(LOW_HZ), mel(HIGH_HZ));
        let delta = (hi - lo) / (NUM_BINS + 1) as f32;
        let bin_hz = 16_000.0 / FFT_LEN as f32;
        let filters = (0..NUM_BINS).map(|b| {
            let (left, center, right) = (lo + b as f32 * delta, lo + (b + 1) as f32 * delta, lo + (b + 2) as f32 * delta);
            let mut first = None;
            let mut w = vec![];
            for i in 0..FFT_LEN / 2 {
                let m = mel(bin_hz * i as f32);
                if m > left && m < right {
                    first.get_or_insert(i);
                    w.push(if m <= center { (m - left) / (center - left) } else { (right - m) / (right - center) });
                }
            }
            (first.unwrap_or(0), w)
        }).collect();
        Self { window, filters, fft: FftPlanner::new().plan_fft_forward(FFT_LEN) }
    }

    /// `audio`: 16 kHz mono em [-1, 1]. Devolve `frames × NUM_BINS` (linha a linha).
    pub fn compute(&self, audio: &[f32]) -> Vec<[f32; NUM_BINS]> {
        if audio.len() < FRAME_LEN { return vec![]; }
        let n = 1 + (audio.len() - FRAME_LEN) / FRAME_SHIFT;
        let mut out = Vec::with_capacity(n);
        let mut frame = [0f32; FRAME_LEN];
        let mut buf = vec![Complex32::new(0.0, 0.0); FFT_LEN];
        for t in 0..n {
            let src = &audio[t * FRAME_SHIFT..t * FRAME_SHIFT + FRAME_LEN];
            for (d, s) in frame.iter_mut().zip(src) { *d = s * 32_768.0; }
            let mean = frame.iter().sum::<f32>() / FRAME_LEN as f32;
            for s in frame.iter_mut() { *s -= mean; }
            for i in (1..FRAME_LEN).rev() { frame[i] -= PREEMPH * frame[i - 1]; }
            frame[0] -= PREEMPH * frame[0];
            for (i, c) in buf.iter_mut().enumerate() {
                *c = Complex32::new(if i < FRAME_LEN { frame[i] * self.window[i] } else { 0.0 }, 0.0);
            }
            self.fft.process(&mut buf);
            let power: Vec<f32> = buf[..FFT_LEN / 2].iter().map(|c| c.norm_sqr()).collect();
            let mut row = [0f32; NUM_BINS];
            for (r, (first, w)) in row.iter_mut().zip(&self.filters) {
                let e: f32 = w.iter().zip(&power[*first..]).map(|(a, p)| a * p).sum();
                *r = e.max(f32::EPSILON).ln();
            }
            out.push(row);
        }
        out
    }
}

/// Normalização de média por filtro (CMN), como no WeSpeaker.
pub fn cmn(feats: &mut [[f32; NUM_BINS]]) {
    if feats.is_empty() { return; }
    let mut mean = [0f32; NUM_BINS];
    for row in feats.iter() { for (m, v) in mean.iter_mut().zip(row) { *m += v; } }
    for m in mean.iter_mut() { *m /= feats.len() as f32; }
    for row in feats.iter_mut() { for (v, m) in row.iter_mut().zip(&mean) { *v -= m; } }
}
```

Run: `cargo test -p gow-meet-core --lib fbank::`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 3: Testes de `speaker` (falhando)**

`crates/core/src/speaker.rs` (por enquanto só os testes):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_cosine() {
        let mut v = vec![3.0, 4.0];
        assert!(normalize(&mut v));
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert!(!normalize(&mut [0.0, 0.0]));
        assert!(!normalize(&mut [f32::NAN, 1.0]));
        assert!((cosine(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), -1.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), -1.0);
    }

    #[test]
    fn embedder_is_send() {
        fn send<T: Send>() {}
        send::<TractEmbedder>();
    }
}
```

`crates/core/src/lib.rs`: acrescente `pub mod speaker;` depois de `pub mod session;`.

Run: `cargo test -p gow-meet-core --lib speaker::`
Expected: erro de compilação (`cannot find function normalize`, `cannot find type TractEmbedder`).

- [ ] **Step 4: Implementar `SpeakerEmbedder` e `TractEmbedder`**

No topo de `crates/core/src/speaker.rs`, antes dos testes:
```rust
//! Impressão de voz de uma fala: vetor de `f32` com norma 1. Fica só na memória da
//! reunião e em `voices.json`; nunca em sessões, logs ou na interface.
use crate::fbank::{cmn, Fbank, NUM_BINS};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::sync::Arc;
use tract_onnx::prelude::*;

pub trait SpeakerEmbedder: Send {
    /// `audio`: 16 kHz mono em [-1, 1]. Devolve a impressão normalizada (norma 1).
    fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>>;
}

/// Normaliza para norma 1. `false` (vetor intocado) se a norma é zero ou não finita.
pub fn normalize(v: &mut [f32]) -> bool {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !n.is_finite() || n < 1e-6 { return false; }
    for x in v.iter_mut() { *x /= n; }
    true
}

/// Semelhança de cosseno; -1 para tamanhos diferentes ou vetores nulos.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() { return -1.0; }
    let (mut ab, mut aa, mut bb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa <= 0.0 || bb <= 0.0 { return -1.0; }
    ab / (aa.sqrt() * bb.sqrt())
}

/// Modelo escolhido na Task 1 (id em `models.json`).
pub const DEFAULT_SPEAKER_MODEL: &str = "wespeaker-ecapa512-lm";

/// Abaixo de 0,5 s o modelo não tem quadros suficientes para uma impressão confiável.
pub const MIN_EMBED_SAMPLES: usize = 8_000;

/// Modelo WeSpeaker em ONNX (entrada `feats` [1, T, 80], saída [1, D]) rodando no tract.
pub struct TractEmbedder {
    model: Arc<TypedRunnableModel>,
    fbank: Fbank,
}

impl TractEmbedder {
    pub fn new(model_path: &Path) -> Result<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(model_path)
            .context("ler o modelo de voz")?
            .into_optimized()
            .context("otimizar o modelo de voz")?
            .into_runnable()
            .context("preparar o modelo de voz")?;
        Ok(Self { model, fbank: Fbank::new() })
    }
}

impl SpeakerEmbedder for TractEmbedder {
    fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
        if audio.len() < MIN_EMBED_SAMPLES { bail!("fala curta demais para impressão de voz"); }
        let mut feats = self.fbank.compute(audio);
        cmn(&mut feats);
        let frames = feats.len();
        let flat: Vec<f32> = feats.into_iter().flatten().collect();
        let input = Tensor::from_shape(&[1, frames, NUM_BINS], &flat)?;
        let out = self.model.run(tvec!(input.into()))?;
        let mut v: Vec<f32> = out[0].to_plain_array_view::<f32>()?.iter().copied().collect();
        if !normalize(&mut v) { bail!("impressão de voz inválida"); }
        Ok(v)
    }
}
```

Run: `cargo test -p gow-meet-core --lib speaker::`
Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Teste de integração com as vozes do `say`**

`crates/core/tests/speaker_say.rs`:
```rust
//! Identificação de falantes com vozes do `say` (frases fictícias). Rode com:
//! bash scripts/gen-voice-fixtures.sh
//! GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
//! GOW_MEET_VOICES=$PWD/target/voice-fixtures \
//!   cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
use gow_meet_core::speaker::{cosine, normalize, SpeakerEmbedder, TractEmbedder};
use gow_meet_core::wav::read_wav_16k_mono;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

/// (voz, índice da frase) → áudio. Arquivos `<Voz>_<i>.wav`.
fn fixtures() -> BTreeMap<(String, usize), Vec<f32>> {
    let dir = PathBuf::from(std::env::var("GOW_MEET_VOICES").expect("defina GOW_MEET_VOICES"));
    std::fs::read_dir(&dir).unwrap().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        let (voice, i) = name.strip_suffix(".wav")?.rsplit_once('_')?;
        Some(((voice.to_string(), i.parse().ok()?), read_wav_16k_mono(&e.path()).unwrap()))
    }).collect()
}

fn model() -> PathBuf {
    PathBuf::from(std::env::var("GOW_MEET_SPEAKER_MODEL").expect("defina GOW_MEET_SPEAKER_MODEL"))
}

#[test]
#[ignore]
fn embeddings_tell_say_voices_apart() {
    let mut emb = TractEmbedder::new(&model()).unwrap();
    let mut times = vec![];
    let embs: Vec<(String, Vec<f32>)> = fixtures().into_iter().map(|((voice, _), audio)| {
        let t0 = Instant::now();
        let e = emb.embed(&audio).unwrap();
        times.push(t0.elapsed().as_millis() as u64);
        assert!((e.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-4, "impressão sem norma 1");
        (voice, e)
    }).collect();
    // Deixa-um-de-fora: cada fala comparada aos centroides das vozes (sem ela mesma).
    let mut wrong = 0;
    for (i, (voice, e)) in embs.iter().enumerate() {
        let mut sums: BTreeMap<&str, Vec<f32>> = BTreeMap::new();
        for (j, (v, x)) in embs.iter().enumerate() {
            if i == j { continue; }
            let s = sums.entry(v.as_str()).or_insert_with(|| vec![0.0; x.len()]);
            for (a, b) in s.iter_mut().zip(x) { *a += b; }
        }
        let best = sums.into_iter().map(|(v, mut c)| { normalize(&mut c); (v, cosine(&c, e)) })
            .max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
        if best != voice { wrong += 1; eprintln!("{voice} → {best}"); }
    }
    times.sort_unstable();
    let p90 = times[(times.len() * 9).div_ceil(10) - 1];
    eprintln!("{} falas, {wrong} erradas | impressão: mediana {} ms, p90 {p90} ms", embs.len(), times[times.len() / 2]);
    assert_eq!(wrong, 0);
    assert!(p90 <= 150, "p90 = {p90} ms");
}
```

Run:
```bash
GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
GOW_MEET_VOICES=$PWD/target/voice-fixtures \
  cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
```
Expected: `32 falas, 0 erradas | impressão: mediana ~35 ms, p90 ~38 ms` e `test result: ok. 1 passed`. Anote o p90 na coluna "tract/Rust" de `docs/modelo-de-voz.md` (para os outros candidatos, troque o caminho em `GOW_MEET_SPEAKER_MODEL`; no levantamento: CAM++ 76 ms, ResNet34 206 ms — reprova no limite de 150 ms).

- [ ] **Step 6: Rodar a suíte inteira**

Run: `cargo test -p gow-meet-core`
Expected: tudo passa; os testes `speaker_say` aparecem como `ignored`.

- [ ] **Step 7: Commit**

```bash
git add crates/core/src/fbank.rs crates/core/src/speaker.rs crates/core/src/lib.rs \
  crates/core/tests/speaker_say.rs docs/modelo-de-voz.md
git commit -m "Adiciona a impressão de voz com filterbank Kaldi e tract" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 3: `VoiceLibrary` e `SpeakerTracker`

**Files:**
- Create: `crates/core/src/voices.rs`, `crates/core/src/tracker.rs`
- Modify: `crates/core/src/session.rs` (só os tipos de `speakers.json`), `crates/core/src/lib.rs`, `crates/core/tests/speaker_say.rs`

**Interfaces:**
- Consumes: `speaker::{normalize, cosine}`, `speaker::TractEmbedder`, `SpeakerEmbedder` (Task 2); `session::write_atomic(path: &Path, bytes: &[u8]) -> Result<()>` (etapa 1); `fixtures()` e `model()` de `tests/speaker_say.rs` (Task 2).
- Produces:
  - `session::SpeakerEntry { pub label: String, pub voice_id: Option<String> }` (serde; `voice_id` omitido quando `None`) e `session::Speakers = BTreeMap<String, SpeakerEntry>` (conteúdo de `speakers.json`: grupo `"v1"` → entrada).
  - `voices::VOICES_FILE: &str` = `"voices.json"`.
  - `voices::Voice { id, name, model: String, embedding: Vec<f32>, created: u64, updated: u64, count: u64 }` (só em `voices.json`).
  - `voices::VoiceSummary { id: String, name: String, updated: u64, count: u64 }` (`Serialize`; é o item de `list_voices`, sem impressão).
  - `voices::clean_name(name: &str) -> Result<String>` (apara; 1–60 caracteres).
  - `voices::VoiceLibrary`: `empty(path: &Path) -> Self`; `load(path: &Path, now: u64) -> (Self, Option<PathBuf>)` (o `Some` é a cópia preservada de um arquivo corrompido); `read_summaries(path: &Path) -> Result<Vec<VoiceSummary>>` (sem efeitos; corrompido = erro); `save(&self) -> Result<()>`; `len`, `is_empty`, `get(&self, id) -> Option<&Voice>`; `summaries(&self) -> Vec<VoiceSummary>` (mais recentes primeiro); `best_match(&self, model, emb) -> Option<(&Voice, f32)>`; `find_name(&self, model, name) -> Option<&Voice>`; `add_to(&mut self, id, sum: &[f32], count: u64, now: u64) -> Result<()>`; `add_named(&mut self, model, name, sum: &[f32], count: u64, now: u64) -> Result<String>`; `rename(&mut self, id, name) -> Result<()>`; `delete(&mut self, id) -> Result<()>`; `delete_all(&mut self) -> Result<()>`.
  - `tracker::TrackerConfig { pub library_threshold: f32, pub group_threshold: f32, pub min_speech_ms: u64 }` (`Default` = 0,70 / 0,65 / 1 000; `PartialEq`).
  - `tracker::Assignment { pub voice: String, pub label: String }`.
  - `tracker::SpeakerTracker`: `new(cfg: TrackerConfig, model: &str, library: VoiceLibrary) -> Self`; `min_speech_ms(&self) -> u64`; `assign(&mut self, emb: Option<&[f32]>, speech_ms: u64) -> Option<Assignment>`; `peek(&self, emb: &[f32]) -> Option<String>`; `note_embed_failure(&mut self)`; `rename(&mut self, voice: &str, name: &str, now: u64) -> Result<Assignment>`; `finish(&mut self, now: u64)`; `save_library(&self) -> Result<()>`; `speakers(&self) -> Speakers`.

- [ ] **Step 1: Tipos de `speakers.json` na sessão**

Em `crates/core/src/session.rs`, logo depois do `impl Speaker { … }`:
```rust
/// Entrada de `speakers.json`: rótulo atual do grupo e a voz guardada a que ele aponta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerEntry {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
}

/// `speakers.json`: grupo (`"v1"`) → rótulo. Sessões da etapa 1 não têm o arquivo.
pub type Speakers = BTreeMap<String, SpeakerEntry>;
```

Run: `cargo build -p gow-meet-core`
Expected: compila (o `BTreeMap` já está importado em `session.rs`).

- [ ] **Step 2: Testes da biblioteca de vozes (falhando)**

`crates/core/src/voices.rs` (por enquanto só os testes):
```rust
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
        std::fs::write(d.path().join("voices.json.corrompido-5"), "{").unwrap();
        std::fs::write(d.path().join("outro.json"), "{}").unwrap();
        l.delete_all().unwrap();
        assert!(l.is_empty());
        let left: Vec<String> = std::fs::read_dir(d.path()).unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, vec!["outro.json".to_string()]);
    }

    #[test]
    fn corrupt_file_gives_empty_library_and_is_preserved() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(VOICES_FILE);
        std::fs::write(&path, "{ \"v\": 1, \"voices\": [ { quebrado").unwrap();
        assert!(VoiceLibrary::read_summaries(&path).is_err());
        let (l, backup) = VoiceLibrary::load(&path, 77);
        assert!(l.is_empty());
        let backup = backup.unwrap();
        assert_eq!(backup, d.path().join("voices.json.corrompido-77"));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ \"v\": 1, \"voices\": [ { quebrado");
        // A próxima gravação não sobrescreve o original preservado.
        l.save().unwrap();
        assert!(backup.exists() && path.exists());
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
}
```

`crates/core/src/lib.rs`: acrescente `pub mod voices;` depois de `pub mod translate;`.

Run: `cargo test -p gow-meet-core --lib voices::`
Expected: erro de compilação (`cannot find type VoiceLibrary`, `cannot find value VOICES_FILE`).

- [ ] **Step 3: Implementar a biblioteca**

No topo de `crates/core/src/voices.rs`, antes dos testes:
```rust
//! Vozes guardadas: `voices.json` na pasta LOCAL do app (fora do perfil que sincroniza).
//! As impressões de voz existem só neste arquivo e na memória da reunião; a interface
//! recebe `VoiceSummary`, sem impressão. Logs: só contagens e ids, nunca nomes.
use crate::session::write_atomic;
use crate::speaker::{cosine, normalize};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl VoiceLibrary {
    pub fn empty(path: &Path) -> Self { Self { path: path.to_path_buf(), voices: vec![] } }

    /// Arquivo ausente: biblioteca vazia. Ilegível ou inválido: biblioteca vazia e o
    /// original renomeado para `voices.json.corrompido-<now>` (devolvido), para que a
    /// próxima gravação não o sobrescreva. O log não leva o conteúdo.
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
            Ok(f) => (Self { path: path.to_path_buf(), voices: f.voices }, None),
            Err(()) => {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(VOICES_FILE);
                let backup = path.with_file_name(format!("{name}.corrompido-{now}"));
                match fs::rename(path, &backup) {
                    Ok(()) => tracing::warn!("biblioteca de vozes vazia; original preservado"),
                    Err(e) => tracing::error!(kind = ?e.kind(), "não foi possível preservar o voices.json inválido"),
                }
                (Self::empty(path), Some(backup))
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
        Ok(Self { path: path.to_path_buf(), voices: f.voices }.summaries())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() { fs::create_dir_all(dir)?; }
        let file = VoicesFile { v: 1, voices: self.voices.clone() };
        write_atomic(&self.path, &serde_json::to_vec(&file)?)
    }

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
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        let v = self.voices.iter_mut().find(|v| v.id == id).context("voz guardada inexistente")?;
        v.name = name;
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let before = self.voices.len();
        self.voices.retain(|v| v.id != id);
        if self.voices.len() == before { bail!("voz guardada inexistente"); }
        Ok(())
    }

    /// Esvazia a biblioteca e apaga do disco o `voices.json`, o temporário e as cópias
    /// preservadas de arquivos corrompidos (que também contêm impressões de voz).
    pub fn delete_all(&mut self) -> Result<()> {
        self.voices.clear();
        let name = self.path.file_name().and_then(|n| n.to_str()).unwrap_or(VOICES_FILE).to_string();
        let prefix = format!("{name}.");
        if let Some(dir) = self.path.parent() {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if file == name || file.starts_with(&prefix) {
                    fs::remove_file(entry.path()).context("apagar o arquivo de vozes")?;
                }
            }
        }
        Ok(())
    }

    fn new_id(&self, now: u64) -> String {
        (0u32..).map(|n| format!("{now:x}-{n}")).find(|id| self.get(id).is_none()).expect("id livre")
    }
}
```

Run: `cargo test -p gow-meet-core --lib voices::`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 4: Testes do rastreador (falhando)**

`crates/core/src/tracker.rs` (por enquanto só os testes). Impressões falsas em 3 dimensões, sem modelo:
```rust
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
}
```

`crates/core/src/lib.rs`: acrescente `pub mod tracker;` depois de `pub mod speaker;`.

Run: `cargo test -p gow-meet-core --lib tracker::`
Expected: erro de compilação (`cannot find type SpeakerTracker`).

- [ ] **Step 5: Implementar o rastreador**

No topo de `crates/core/src/tracker.rs`, antes dos testes:
```rust
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

    /// Rótulo provável de uma fala ainda aberta (parcial), sem criar nem mudar grupos.
    pub fn peek(&self, emb: &[f32]) -> Option<String> {
        if let Some((v, s)) = self.library.best_match(&self.model, emb) {
            if s >= self.cfg.library_threshold {
                return Some(self.group_for(&v.id).map_or(v.name.clone(), |i| self.groups[i].label.clone()));
            }
        }
        self.best_group(emb).map(|i| self.groups[i].label.clone())
    }

    pub fn note_embed_failure(&mut self) { self.embed_failures += 1; }

    /// Dá nome ao grupo `voice`: grava a impressão do grupo na biblioteca (voz nova ou
    /// somada à de mesmo nome) e passa a mostrar o nome. Uma voz guardada com que o grupo
    /// tinha sido reconhecido não muda.
    pub fn rename(&mut self, voice: &str, name: &str, now: u64) -> Result<Assignment> {
        let name = crate::voices::clean_name(name)?;
        let i = self.groups.iter().position(|g| g.voice == voice).context("voz inexistente nesta reunião")?;
        let same = self.library.find_name(&self.model, &name).map(|v| v.id.clone());
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
            voice, label, voice_id: recognized.clone(), recognized,
            sum: vec![0.0; dim], count: 0, pending: vec![0.0; dim], pending_count: 0,
        });
        self.groups.len() - 1
    }
}
```

Run: `cargo test -p gow-meet-core --lib tracker::`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 6: Rastreador com as vozes do `say` (duas reuniões)**

Em `crates/core/tests/speaker_say.rs`, troque os `use` do topo por:
```rust
use gow_meet_core::speaker::{cosine, normalize, SpeakerEmbedder, TractEmbedder};
use gow_meet_core::tracker::{SpeakerTracker, TrackerConfig};
use gow_meet_core::voices::{VoiceLibrary, VOICES_FILE};
use gow_meet_core::wav::read_wav_16k_mono;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;
```
e acrescente no fim do arquivo:
```rust
/// Ordem de reunião: as vozes se alternam a cada frase.
fn meeting(f: &BTreeMap<(String, usize), Vec<f32>>, sentences: std::ops::Range<usize>) -> Vec<(String, &Vec<f32>)> {
    let voices: BTreeSet<&String> = f.keys().map(|(v, _)| v).collect();
    sentences.flat_map(|i| voices.iter().map(move |v| ((*v).clone(), i)).collect::<Vec<_>>())
        .filter_map(|(v, i)| f.get(&(v.clone(), i)).map(|a| (v, a)))
        .collect()
}

#[test]
#[ignore]
fn each_say_voice_gets_its_own_group_and_is_recognized_next_time() {
    let mut emb = TractEmbedder::new(&model()).unwrap();
    let f = fixtures();
    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);

    // Reunião 1: cada voz num grupo próprio, e cada grupo com uma voz só.
    let mut t = SpeakerTracker::new(TrackerConfig::default(), "teste", VoiceLibrary::empty(&lib_path));
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (voice, audio) in meeting(&f, 0..4) {
        let e = emb.embed(audio).unwrap();
        let a = t.assign(Some(&e), audio.len() as u64 / 16).unwrap();
        groups.entry(a.voice).or_default().insert(voice);
    }
    eprintln!("reunião 1: {groups:?}");
    let n_voices = f.keys().map(|(v, _)| v).collect::<BTreeSet<_>>().len();
    assert_eq!(groups.len(), n_voices, "grupos ≠ vozes: {groups:?}");
    assert!(groups.values().all(|s| s.len() == 1), "grupo com mais de uma voz: {groups:?}");

    // Nomeia cada grupo com o nome da voz do `say` (fictício) e encerra.
    for (g, voices) in &groups { t.rename(g, voices.iter().next().unwrap(), 1).unwrap(); }
    t.finish(2);
    t.save_library().unwrap();

    // Reunião 2, outras frases: cada voz reconhecida pelo nome.
    let (lib, corrupt) = VoiceLibrary::load(&lib_path, 3);
    assert!(corrupt.is_none());
    let mut t = SpeakerTracker::new(TrackerConfig::default(), "teste", lib);
    let mut wrong = 0;
    let all = meeting(&f, 4..8);
    for (voice, audio) in &all {
        let e = emb.embed(audio).unwrap();
        let a = t.assign(Some(&e), audio.len() as u64 / 16).unwrap();
        if &a.label != voice { wrong += 1; eprintln!("{voice} → {}", a.label); }
    }
    eprintln!("reunião 2: {wrong}/{} erradas", all.len());
    assert_eq!(wrong, 0);
}
```

Run:
```bash
GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
GOW_MEET_VOICES=$PWD/target/voice-fixtures \
  cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
```
Expected:
```text
reunião 1: {"v1": {"Daniel"}, "v2": {"Karen"}, "v3": {"Moira"}, "v4": {"Samantha"}}
reunião 2: 0/16 erradas
test result: ok. 2 passed
```
(A ordem dos grupos acompanha a ordem alfabética das vozes; com outras vozes os nomes mudam.)

- [ ] **Step 7: Suíte inteira e commit**

Run: `cargo test -p gow-meet-core`
Expected: tudo passa.

```bash
git add crates/core/src/voices.rs crates/core/src/tracker.rs crates/core/src/session.rs \
  crates/core/src/lib.rs crates/core/tests/speaker_say.rs
git commit -m "Adiciona a biblioteca de vozes e o rastreador de falantes" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 4: Formato da sessão — `voice`, `speakers.json`, Markdown e recuperação

**Files:**
- Modify: `crates/core/src/session.rs`

**Interfaces:**
- Consumes: `SpeakerEntry`, `Speakers` (Task 3); `write_atomic`, `replay`, `recover_unfinished`, `SessionWriter` (etapa 1).
- Produces:
  - `Speaker { pub source: SpeakerSource, pub label: String, pub voice: Option<String> }` (`voice` omitido no JSON quando `None`; ausente em registros antigos = `None`).
  - `Speaker::tracked(voice: &str, label: &str) -> Speaker` (fonte `System`); `Speaker::mic()`/`Speaker::system()` continuam iguais (sem `voice`).
  - `session::write_speakers(dir: &Path, speakers: &Speakers) -> Result<()>` (atômico, `dir/speakers.json`); `session::read_speakers(dir: &Path) -> Speakers` (ausente ou inválido = vazio); `session::label_for(line: &Line, speakers: &Speakers) -> &str`.
  - `SessionWriter::set_speakers(&mut self, speakers: Speakers) -> Result<()>` (guarda em memória e grava; se falhar, a memória vale para o Markdown e `finish` tenta de novo).
  - `render_markdown(meta: &Meta, lines: &[Line], speakers: &Speakers) -> String` (assinatura nova).
  - `SessionWriter::finish` e `recover_unfinished` usam os rótulos de `speakers.json`.

- [ ] **Step 1: Testes (falhando)**

Em `crates/core/src/session.rs`, no módulo `tests`, antes do teste `missing_translation_is_omitted_from_json_and_round_trips`:
```rust
    fn tracked(id: &str, t0: u64, voice: &str, label: &str, text: &str) -> Line {
        Line { speaker: Speaker::tracked(voice, label), ..line(id, t0, text, Some("ok")) }
    }

    fn speakers(pairs: &[(&str, &str, Option<&str>)]) -> Speakers {
        pairs.iter().map(|(v, l, id)| (v.to_string(), SpeakerEntry { label: l.to_string(), voice_id: id.map(Into::into) })).collect()
    }

    #[test]
    fn speakers_json_is_atomic_and_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp1")).unwrap();
        let s = speakers(&[("v1", "Ana", Some("abc-0")), ("v2", "Participante 2", None)]);
        w.set_speakers(s.clone()).unwrap();
        let dir = w.dir().to_path_buf();
        assert_eq!(read_speakers(&dir), s);
        let raw = std::fs::read_to_string(dir.join("speakers.json")).unwrap();
        assert!(raw.contains("\"voice_id\": \"abc-0\""));
        assert!(!raw.contains("\"voice_id\": null"));
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn markdown_uses_speakers_json_names_even_for_earlier_lines() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp2")).unwrap();
        w.append(&tracked("s0", 1_000, "v1", "Participante 1", "Hello team.")).unwrap();
        w.append(&tracked("s1", 2_000, "v2", "Participante 2", "Good morning.")).unwrap();
        w.append(&line("m0", 3_000, "Hi.", Some("Oi."))).unwrap();
        // Renomeado depois das falas: só o speakers.json muda.
        w.set_speakers(speakers(&[("v1", "Ana", Some("abc-0")), ("v2", "Participante 2", None)])).unwrap();
        let md = std::fs::read_to_string(w.finish(9_000).unwrap()).unwrap();
        assert!(md.contains("**[00:00:01] Ana:** ok"), "{md}");
        assert!(md.contains("**[00:00:02] Participante 2:** ok"), "{md}");
        assert!(md.contains("**[00:00:03] Participante:** Oi."), "{md}");
    }

    #[test]
    fn old_sessions_without_voice_or_speakers_json_still_work() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("antiga");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("meta.json"), serde_json::to_vec(&meta("antiga")).unwrap()).unwrap();
        // Registro exatamente como a etapa 1 gravava: speaker sem `voice`.
        std::fs::write(dir.join("lines.jsonl"),
            "{\"id\":\"s0\",\"t0\":0,\"t1\":900,\"speaker\":{\"source\":\"system\",\"label\":\"Participante\"},\"text\":\"Hello team.\",\"translation\":\"Olá, equipe.\"}\n").unwrap();
        let lines = replay(&dir.join("lines.jsonl")).unwrap();
        assert_eq!(lines[0].speaker, Speaker::system());
        assert!(read_speakers(&dir).is_empty());
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(dir.join("transcript.md")).unwrap();
        assert!(md.contains("**[00:00:00] Participante:** Olá, equipe."));
        // E a etapa 2 continua gravando sem `voice` para o microfone e o sistema genérico.
        assert!(!serde_json::to_string(&Speaker::mic()).unwrap().contains("voice"));
    }

    #[test]
    fn recovery_uses_names_from_speakers_json() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp3")).unwrap();
        w.append(&tracked("s0", 2_000, "v1", "Participante 1", "Hello team.")).unwrap();
        w.set_speakers(speakers(&[("v1", "Ana", Some("abc-0"))])).unwrap();
        drop(w); // queda: sem finish
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(root.path().join("sp3/transcript.md")).unwrap();
        assert!(md.contains("**[00:00:02] Ana:** ok"), "{md}");
    }

    #[test]
    fn corrupt_speakers_json_falls_back_to_line_labels() {
        let root = tempfile::tempdir().unwrap();
        let mut w = SessionWriter::create(root.path(), meta("sp4")).unwrap();
        w.append(&tracked("s0", 0, "v1", "Participante 1", "Hello team.")).unwrap();
        std::fs::write(w.dir().join("speakers.json"), "{ quebrado").unwrap();
        drop(w);
        recover_unfinished(root.path()).unwrap();
        let md = std::fs::read_to_string(root.path().join("sp4/transcript.md")).unwrap();
        assert!(md.contains("**[00:00:00] Participante 1:** ok"), "{md}");
    }

```

Run: `cargo test -p gow-meet-core --lib session::`
Expected: erro de compilação (`no function or associated item named tracked`, `cannot find function read_speakers`, `no method named set_speakers`).

- [ ] **Step 2: `Speaker.voice` e `speakers.json`**

Em `crates/core/src/session.rs`, troque a struct `Speaker` e o `impl Speaker` por:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Speaker {
    pub source: SpeakerSource,
    pub label: String,
    /// Grupo de voz na reunião (`"v1"`, `"v2"`…). Só em falas do sistema identificadas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
}

impl Speaker {
    pub fn mic() -> Self { Self { source: SpeakerSource::Mic, label: "Você".into(), voice: None } }
    pub fn system() -> Self { Self { source: SpeakerSource::System, label: "Participante".into(), voice: None } }
    /// Fala do sistema atribuída ao grupo `voice`.
    pub fn tracked(voice: &str, label: &str) -> Self {
        Self { source: SpeakerSource::System, label: label.into(), voice: Some(voice.into()) }
    }
}
```

Logo depois de `pub type Speakers = BTreeMap<String, SpeakerEntry>;` (Task 3), acrescente:
```rust
const SPEAKERS_FILE: &str = "speakers.json";

pub fn write_speakers(dir: &Path, speakers: &Speakers) -> Result<()> {
    write_atomic(&dir.join(SPEAKERS_FILE), &serde_json::to_vec_pretty(speakers)?)
}

/// Ausente: vazio (sessão antiga). Inválido: vazio, com aviso sem conteúdo; as falas
/// usam então o rótulo gravado em cada linha.
pub fn read_speakers(dir: &Path) -> Speakers {
    let Ok(bytes) = fs::read(dir.join(SPEAKERS_FILE)) else { return Speakers::new() };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!(line = e.line(), column = e.column(), "speakers.json inválido; usando os rótulos das falas");
        Speakers::new()
    })
}

/// Rótulo da fala: o de `speakers.json` para o grupo, se houver; senão o da própria linha.
pub fn label_for<'a>(line: &'a Line, speakers: &'a Speakers) -> &'a str {
    line.speaker.voice.as_ref().and_then(|v| speakers.get(v)).map_or(line.speaker.label.as_str(), |e| e.label.as_str())
}
```

- [ ] **Step 3: Writer, Markdown e recuperação com os nomes**

Troque `pub struct SessionWriter { dir: PathBuf, meta: Meta, lines: File }` por:
```rust
pub struct SessionWriter { dir: PathBuf, meta: Meta, lines: File, speakers: Speakers }
```
Em `SessionWriter::create`, troque `Ok(Self { dir, meta, lines })` por:
```rust
        Ok(Self { dir, meta, lines, speakers: Speakers::new() })
```
Troque `pub fn finish(...)` e `fn close_session(...)` inteiros por:
```rust
    /// Guarda os rótulos em memória e grava `speakers.json`. Se a gravação falhar, a
    /// memória já vale para o Markdown e a próxima chamada (ou `finish`) tenta de novo.
    pub fn set_speakers(&mut self, speakers: Speakers) -> Result<()> {
        self.speakers = speakers;
        write_speakers(&self.dir, &self.speakers)
    }

    pub fn finish(mut self, ended_ms: u64) -> Result<PathBuf> {
        self.lines.sync_all()?;
        self.meta.ended = Some(ended_ms);
        if !self.speakers.is_empty() {
            if let Err(e) = write_speakers(&self.dir, &self.speakers) {
                tracing::warn!("speakers.json não gravado ao encerrar: {e:#}");
            }
        }
        close_session(&self.dir, &self.meta, &self.speakers)
    }
}

fn close_session(dir: &Path, meta: &Meta, speakers: &Speakers) -> Result<PathBuf> {
    write_meta(dir, meta)?;
    let lines = replay(&dir.join("lines.jsonl"))?;
    let md = dir.join("transcript.md");
    write_atomic(&md, render_markdown(meta, &lines, speakers).as_bytes())?;
    Ok(md)
}
```
Em `render_markdown`, troque a assinatura por `pub fn render_markdown(meta: &Meta, lines: &[Line], speakers: &Speakers) -> String {` e, no laço, `l.speaker.label` por `label_for(l, speakers)`:
```rust
        out.push_str(&format!("**[{}] {}:** {}\n> {}\n\n", clock(l.t0), label_for(l, speakers), tr, l.text));
```
Em `recover_unfinished`, troque `close_session(&dir, &meta)` por:
```rust
            close_session(&dir, &meta, &read_speakers(&dir))
```

Run: `cargo test -p gow-meet-core --lib session::`
Expected: `test result: ok. 15 passed` (10 da etapa 1 + 5 novos).

- [ ] **Step 4: Suíte inteira e commit**

Run: `cargo test -p gow-meet-core && cargo build -p gow-meet-cli`
Expected: tudo passa; o CLI compila sem mudanças (só usa `Speaker` pelos construtores).

```bash
git add crates/core/src/session.rs
git commit -m "Grava o grupo de voz das falas e os nomes em speakers.json" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 5: Integração no pipeline

**Files:**
- Modify: `crates/core/src/pipeline.rs`, `crates/core/Cargo.toml` (dev-dependency), `crates/cli/src/main.rs`, `crates/core/tests/speaker_say.rs`, `src-tauri/src/lib.rs` (só o evento: o `match` do `TauriSink` é exaustivo)
- Create: `crates/core/tests/voice_privacy.rs`

**Interfaces:**
- Consumes: `SpeakerEmbedder`, `TractEmbedder` (Task 2); `SpeakerTracker` (`assign`, `peek`, `rename`, `finish`, `save_library`, `speakers`, `min_speech_ms`, `note_embed_failure`), `TrackerConfig`, `VoiceLibrary`, `VOICES_FILE` (Task 3); `Speaker::tracked`, `Speakers`, `SessionWriter::set_speakers`, `read_speakers` (Task 4).
- Produces:
  - `pipeline::SpeakerId { pub embedder: Box<dyn SpeakerEmbedder>, pub tracker: SpeakerTracker }`.
  - `Pipeline::start_with_speakers(cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>, speakers: Option<SpeakerId>, writer: SessionWriter, sink: Arc<dyn Sink>) -> Pipeline` (`Pipeline::start` continua igual e chama esta com `None`).
  - `Pipeline::rename_voice(&self, voice: &str, name: &str) -> anyhow::Result<String>` (devolve o rótulo novo; erro sem efeito se a identificação está desligada, a voz não existe ou o nome é vazio).
  - `PipelineEvent::Speaker { voice: String, label: String }`.
  - `pipeline::SPEAKER_PANIC_MESSAGE: &str`.
  - Fala final do sistema sai em `PipelineEvent::Final` já com `speaker.voice = Some("vN")` e o rótulo do rastreador; microfone sempre `Speaker::mic()`.
  - No app: `UiEvent::Speaker { voice: String, label: String }`, serializado no evento `"gow-meet"` como `{"kind":"speaker","voice":…,"label":…}`; o `TauriSink` converte `PipelineEvent::Speaker` nele.

- [ ] **Step 1: Testes do pipeline (falhando)**

Em `crates/core/src/pipeline.rs`, no módulo `tests`, antes de `fn job(source: SpeakerSource, …)`:
```rust
    // ---------- Identificação de quem fala (etapa 2) ----------

    use crate::tracker::{SpeakerTracker, TrackerConfig};
    use crate::voices::{VoiceLibrary, VOICES_FILE};

    /// Fala com amplitude própria: o embedder falso distingue "pessoas" pela energia.
    fn voice_tone(amp: f32, ms: usize) -> Vec<f32> { (0..ms * MS).map(|i| amp * (i as f32 * 0.17).sin()).collect() }
    const QUIET: f32 = 0.1;
    const LOUD: f32 = 0.4;

    /// Impressão falsa: [1,0,0] para fala baixa, [0,1,0] para fala alta.
    struct EnergyEmbedder;
    impl SpeakerEmbedder for EnergyEmbedder {
        fn embed(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
            let rms = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len().max(1) as f32).sqrt();
            Ok(if rms < 0.15 { vec![1.0, 0.0, 0.0] } else { vec![0.0, 1.0, 0.0] })
        }
    }
    struct PanicEmbedder;
    impl SpeakerEmbedder for PanicEmbedder {
        fn embed(&mut self, _a: &[f32]) -> Result<Vec<f32>> { panic!("falha simulada no modelo de voz") }
    }

    fn speaker_id(lib_dir: &std::path::Path, embedder: Box<dyn SpeakerEmbedder>) -> SpeakerId {
        let library = VoiceLibrary::empty(&lib_dir.join(VOICES_FILE));
        SpeakerId { embedder, tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) }
    }

    fn start_tracked(lib_dir: &std::path::Path, embedder: Box<dyn SpeakerEmbedder>)
        -> (tempfile::TempDir, Pipeline, Arc<ChanSink>, Receiver<PipelineEvent>) {
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(),
            app_version: "0".into() };
        let writer = SessionWriter::create(root.path(), meta).unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let sink = Arc::new(ChanSink(Mutex::new(vec![]), tx));
        let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr("Hello team.")),
            Box::new(Prefix), Some(speaker_id(lib_dir, embedder)), writer, sink.clone());
        (root, p, sink, rx)
    }

    fn utterance(p: &Pipeline, source: SpeakerSource, amp: f32) {
        p.feed(source, &[voice_tone(amp, 1_500), silence(1_000)].concat());
    }

    fn finals(ev: &[PipelineEvent]) -> Vec<Line> {
        ev.iter().filter_map(|e| match e { PipelineEvent::Final(l) => Some(l.clone()), _ => None }).collect()
    }

    #[test]
    fn system_final_already_has_voice_and_label_and_mic_stays_voce() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, _rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::Mic, LOUD);
        utterance(&p, SpeakerSource::System, LOUD);
        utterance(&p, SpeakerSource::System, QUIET);
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let got: Vec<(String, String, Option<String>)> = finals(&ev).into_iter()
            .map(|l| (l.id, l.speaker.label, l.speaker.voice)).collect();
        assert_eq!(got, vec![
            ("s0".into(), "Participante 1".into(), Some("v1".into())),
            ("m0".into(), "Você".into(), None),
            ("s1".into(), "Participante 2".into(), Some("v2".into())),
            ("s2".into(), "Participante 1".into(), Some("v1".into())),
        ]);
        // Nenhuma fala final do sistema sai como "Participante" genérico para depois mudar.
        assert!(!ev.iter().any(|e| matches!(e, PipelineEvent::Final(l) if l.speaker.label == "Participante")));
    }

    #[test]
    fn rename_emits_speaker_and_names_earlier_and_later_lines() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(l) if l.id == "s0"));
        assert_eq!(p.rename_voice("v1", " Ana ").unwrap(), "Ana");
        assert!(p.rename_voice("v9", "Bruno").is_err());
        assert!(p.rename_voice("v1", "   ").is_err());
        utterance(&p, SpeakerSource::System, QUIET);
        let md = std::fs::read_to_string(p.finish(10_000).unwrap()).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        let sp: Vec<_> = ev.iter().filter(|e| matches!(e, PipelineEvent::Speaker { .. })).cloned().collect();
        assert_eq!(sp, vec![PipelineEvent::Speaker { voice: "v1".into(), label: "Ana".into() }]);
        let f = finals(&ev);
        assert_eq!(f[1].speaker, Speaker::tracked("v1", "Ana"));
        // Markdown: a fala anterior à renomeação também sai com o nome.
        assert_eq!(md.matches("] Ana:**").count(), 2, "{md}");
        let dir = _d.path().join("t");
        let speakers = crate::session::read_speakers(&dir);
        assert_eq!(speakers["v1"].label, "Ana");
        assert!(speakers["v1"].voice_id.is_some());
        let (library, _) = VoiceLibrary::load(&lib.path().join(VOICES_FILE), 0);
        let s = library.summaries();
        assert_eq!((s.len(), s[0].name.as_str(), s[0].count), (1, "Ana", 2));
    }

    #[test]
    fn named_voice_is_recognized_in_the_next_meeting() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, _s, rx) = start_tracked(lib.path(), Box::new(EnergyEmbedder));
        utterance(&p, SpeakerSource::System, LOUD);
        wait_for(&rx, |e| matches!(e, PipelineEvent::Final(_)));
        p.rename_voice("v1", "Bruno").unwrap();
        p.finish(10_000).unwrap();
        let root = tempfile::tempdir().unwrap();
        let meta = Meta { v: 1, id: "t2".into(), title: "Teste".into(), started: 0, ended: None,
            source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
        let (library, _) = VoiceLibrary::load(&lib.path().join(VOICES_FILE), 0);
        let sid = SpeakerId { embedder: Box::new(EnergyEmbedder),
                              tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) };
        let sink = Arc::new(Collect::default());
        let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr("Hello again.")),
            Box::new(Prefix), Some(sid), SessionWriter::create(root.path(), meta).unwrap(), sink.clone());
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::System, LOUD);
        p.finish(10_000).unwrap();
        let labels: Vec<String> = finals(&sink.0.lock().unwrap()).into_iter().map(|l| l.speaker.label).collect();
        assert_eq!(labels, vec!["Participante 1".to_string(), "Bruno".to_string()]);
    }

    #[test]
    fn rename_without_identification_is_an_error() {
        let (_d, _ev, _lines) = run("Hello team.", Box::new(Prefix), |p| {
            assert!(p.rename_voice("v1", "Ana").is_err());
        });
    }

    #[test]
    fn embedder_panic_emits_one_error_and_lines_stay_participante() {
        let lib = tempfile::tempdir().unwrap();
        let (_d, p, sink, _rx) = start_tracked(lib.path(), Box::new(PanicEmbedder));
        utterance(&p, SpeakerSource::System, QUIET);
        utterance(&p, SpeakerSource::System, LOUD);
        p.finish(10_000).unwrap();
        let ev = sink.0.lock().unwrap().clone();
        assert_eq!(errors(&ev), vec![SPEAKER_PANIC_MESSAGE.to_string()]);
        let f = finals(&ev);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|l| l.speaker == Speaker::system()));
    }

```

Run: `cargo test -p gow-meet-core --lib pipeline::`
Expected: erro de compilação (`cannot find trait SpeakerEmbedder`, `cannot find struct SpeakerId`, `no function start_with_speakers`).

- [ ] **Step 2: Tipos, `start_with_speakers` e `rename_voice`**

Em `crates/core/src/pipeline.rs`:

Troque os `use` de `session` e `translate` por:
```rust
use crate::session::{Line, SessionWriter, Speaker, SpeakerSource, Speakers};
use crate::speaker::SpeakerEmbedder;
use crate::tracker::SpeakerTracker;
use crate::translate::Translator;
```

No `enum PipelineEvent`, depois de `Dropped { id: String },`:
```rust
    /// Grupo de voz renomeado: todas as falas com `speaker.voice == voice` mostram `label`.
    Speaker { voice: String, label: String },
```

Antes de `struct Store`, acrescente:
```rust
/// Identificação de quem fala no áudio do sistema (etapa 2).
pub struct SpeakerId {
    pub embedder: Box<dyn SpeakerEmbedder>,
    pub tracker: SpeakerTracker,
}

/// Aviso único quando a identificação para por erro interno (pânico no modelo de voz).
pub const SPEAKER_PANIC_MESSAGE: &str =
    "A identificação de quem fala parou por um erro interno; as falas seguem como \"Participante\".";
```

No começo do `impl Store`, antes de `fn append`:
```rust
    /// `speakers.json`: os rótulos ficam na memória do writer mesmo se a gravação falhar;
    /// a próxima mudança e o fim da sessão tentam de novo.
    fn set_speakers(&mut self, speakers: Speakers, sink: &dyn Sink) {
        if let Err(e) = self.writer.set_speakers(speakers) {
            tracing::warn!("speakers.json não gravado: {e:#}");
            sink.emit(PipelineEvent::Error("falha ao gravar os nomes das vozes desta reunião; nova tentativa na próxima mudança".into()));
        }
    }

```

Antes de `pub struct Pipeline`, acrescente:
```rust
/// Rastreador compartilhado entre a thread de transcrição e `rename_voice`. Ordem de
/// locks: rastreador → store → parciais abertas.
type SharedTracker = Arc<Mutex<SpeakerTracker>>;

/// O que a thread de transcrição usa para identificar quem fala.
struct Identify {
    embedder: Box<dyn SpeakerEmbedder>,
    tracker: SharedTracker,
    /// Silêncio de fechamento no fim de cada fala final: fica fora da impressão.
    trailing_ms: u64,
    dead: bool,
}

```

Na `pub struct Pipeline`, depois de `store: Arc<Mutex<Option<Store>>>,`:
```rust
    tracker: Option<SharedTracker>,
    sink: Arc<dyn Sink>,
```

Troque `pub fn start(…) -> Self { … }` inteiro por:
```rust
    pub fn start(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        Self::start_with_speakers(cfg, asr, translator, None, writer, sink)
    }

    /// Como `start`, identificando quem fala no áudio do sistema quando `speakers` vem.
    pub fn start_with_speakers(
        cfg: PipelineConfig, asr: Box<dyn Transcriber>, translator: Box<dyn Translator>,
        speakers: Option<SpeakerId>, writer: SessionWriter, sink: Arc<dyn Sink>,
    ) -> Self {
        let (asr_tx, asr_rx) = unbounded::<AsrJob>();
        let (tr_tx, tr_rx) = unbounded::<TJob>();
        let store = Arc::new(Mutex::new(Some(Store { writer, pending: vec![] })));
        let fed = Arc::new(Mutex::new(HashMap::new()));
        let open: OpenPartials = Arc::new(Mutex::new(HashMap::new()));
        let (identify, tracker) = match speakers {
            Some(SpeakerId { embedder, tracker }) => {
                let shared: SharedTracker = Arc::new(Mutex::new(tracker));
                let id = Identify { embedder, tracker: shared.clone(),
                                    trailing_ms: cfg.segmenter.silence_close_ms, dead: false };
                (Some(id), Some(shared))
            }
            None => (None, None),
        };
        let asr_h = {
            let (store, sink, fed, warn, open) =
                (store.clone(), sink.clone(), fed.clone(), cfg.lag_warn_ms, open.clone());
            std::thread::Builder::new().name("asr".into())
                .spawn(move || asr_loop(asr_rx, asr, identify, tr_tx, store, sink, fed, warn, open)).expect("thread asr")
        };
        let tr_h = {
            let (store, sink, every) = (store.clone(), sink.clone(), Duration::from_millis(cfg.partial_translate_every_ms));
            std::thread::Builder::new().name("translate".into())
                .spawn(move || translate_loop(tr_rx, translator, store, sink, every, open)).expect("thread translate")
        };
        Self { cfg, segmenters: Mutex::new(HashMap::new()), fed, asr_tx: Some(asr_tx),
               asr: Some(asr_h), tr: Some(tr_h), store, tracker, sink }
    }

    /// Dá nome ao grupo `voice` durante a reunião: biblioteca de vozes, `speakers.json` e
    /// evento `Speaker`. Erro (sem efeito) para identificação desligada, voz inexistente
    /// ou nome vazio.
    pub fn rename_voice(&self, voice: &str, name: &str) -> Result<String> {
        let tracker = self.tracker.as_ref().context("a identificação de vozes está desligada nesta reunião")?;
        let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
        let a = t.rename(voice, name, now_ms())?;
        if let Err(e) = t.save_library() {
            tracing::warn!("voices.json não gravado: {e:#}");
            self.sink.emit(PipelineEvent::Error(
                "falha ao gravar as vozes guardadas; o nome vale nesta reunião e será gravado de novo ao encerrar".into()));
        }
        if let Some(s) = self.store.lock().unwrap().as_mut() { s.set_speakers(t.speakers(), self.sink.as_ref()); }
        // Emitido com o rastreador travado: nenhuma fala final do grupo com o rótulo
        // antigo sai depois deste evento.
        self.sink.emit(PipelineEvent::Speaker { voice: a.voice, label: a.label.clone() });
        Ok(a.label)
    }
```

Antes de `impl Pipeline {`, acrescente:
```rust
fn now_ms() -> u64 { chrono::Utc::now().timestamp_millis() as u64 }

```

Em `Pipeline::finish`, troque as três últimas linhas (`let mut writer = store.writer;` … `writer.finish(ended_ms)`) por:
```rust
        let mut writer = store.writer;
        for l in &store.pending { writer.append(l)?; }
        if let Some(tracker) = self.tracker.take() {
            let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
            t.finish(now_ms());
            if let Err(e) = t.save_library() { tracing::warn!("voices.json não gravado ao encerrar: {e:#}"); }
            let speakers = t.speakers();
            if !speakers.is_empty() {
                // Falha aqui não perde os nomes: o Markdown usa os rótulos em memória.
                if let Err(e) = writer.set_speakers(speakers) { tracing::warn!("speakers.json não gravado: {e:#}"); }
            }
        }
        writer.finish(ended_ms)
```

- [ ] **Step 3: Identificação na thread de transcrição**

Antes de `fn asr_loop(`, acrescente:
```rust
impl Identify {
    /// Impressão de voz de `audio` (16 kHz) se a fala tem ao menos `min_ms`; `None` se é
    /// curta, se o cálculo falhou (contado) ou se a identificação morreu por pânico.
    fn embed(&mut self, audio: &[f32], speech_ms: u64, sink: &dyn Sink) -> Option<Vec<f32>> {
        if self.dead { return None; }
        let min_ms = self.tracker.lock().unwrap_or_else(|e| e.into_inner()).min_speech_ms();
        if speech_ms < min_ms { return None; }
        match guarded(|| self.embedder.embed(audio)) {
            Some(Ok(e)) => Some(e),
            Some(Err(e)) => {
                tracing::debug!("impressão de voz falhou: {e}");
                self.tracker.lock().unwrap_or_else(|e| e.into_inner()).note_embed_failure();
                None
            }
            None => {
                self.dead = true;
                tracing::error!("pânico no modelo de voz; identificação desligada nesta sessão");
                sink.emit(PipelineEvent::Error(SPEAKER_PANIC_MESSAGE.into()));
                None
            }
        }
    }
}

```

Troque a assinatura de `asr_loop` por:
```rust
fn asr_loop(
    rx: Receiver<AsrJob>, mut asr: Box<dyn Transcriber>, mut identify: Option<Identify>, tr_tx: Sender<TJob>,
    store: Arc<Mutex<Option<Store>>>, sink: Arc<dyn Sink>,
    fed: Arc<Mutex<HashMap<SpeakerSource, u64>>>, lag_warn_ms: u64, open: OpenPartials,
) {
```

Em `asr_loop`, o trecho atual é:
```rust
            let speaker = speaker_for(job.source);
            if job.is_final {
                let line = Line { id, t0: job.start_ms, t1: job.end_ms, speaker, text,
                                  translation: None, translation_error: false };
                if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                {
                    let mut o = open.lock().unwrap();
                    o.remove(&line.id);
                    sink.emit(PipelineEvent::Final(line.clone()));
                }
                let _ = tr_tx.send(TJob::Final(line));
            } else {
                {
```
Troque exatamente essas linhas (até o `{` que abre o bloco da parcial, logo antes de `let mut o = open.lock().unwrap(); o.insert(…)`) por:
```rust
            let mut speaker = speaker_for(job.source);
            // Só o áudio do sistema passa pela identificação; o microfone é sempre "Você".
            let ident = identify.as_mut().filter(|_| job.source == SpeakerSource::System);
            if job.is_final {
                let mut line = Line { id, t0: job.start_ms, t1: job.end_ms, speaker, text,
                                      translation: None, translation_error: false };
                // O rastreador fica travado até o evento sair: um `rename_voice` concorrente
                // é aplicado antes (rótulo novo aqui) ou depois (evento `Speaker` depois).
                let mut tracked = None;
                if let Some(ident) = ident {
                    let trailing = (ident.trailing_ms * 16) as usize;
                    let audio = &job.audio[..job.audio.len().saturating_sub(trailing)];
                    let speech_ms = (job.end_ms - job.start_ms).saturating_sub(ident.trailing_ms);
                    let emb = ident.embed(audio, speech_ms, sink.as_ref());
                    let mut t = ident.tracker.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(a) = t.assign(emb.as_deref(), speech_ms) {
                        line.speaker = Speaker::tracked(&a.voice, &a.label);
                    }
                    tracked = Some(t);
                }
                if let Some(s) = store.lock().unwrap().as_mut() { s.append(&line, sink.as_ref()); }
                {
                    let mut o = open.lock().unwrap();
                    o.remove(&line.id);
                    sink.emit(PipelineEvent::Final(line.clone()));
                }
                drop(tracked);
                let _ = tr_tx.send(TJob::Final(line));
            } else {
                // Parcial: rótulo provável, sem mudar grupos; a fala final confirma.
                if let Some(ident) = ident {
                    let speech_ms = job.end_ms - job.start_ms;
                    if let Some(emb) = ident.embed(&job.audio, speech_ms, sink.as_ref()) {
                        let t = ident.tracker.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(label) = t.peek(&emb) { speaker.label = label; }
                    }
                }
                {
```
O resto do bloco da parcial (`let mut o = open.lock().unwrap(); o.insert(…); sink.emit(PipelineEvent::Partial { … }) }` e o `tr_tx.send(TJob::Partial { … })`) fica como está.

`crates/cli/src/main.rs`, no `match` de `Term::emit`, troque `PipelineEvent::Dropped { .. } => {}` por:
```rust
            PipelineEvent::Dropped { .. } | PipelineEvent::Speaker { .. } => {}
```

Run: `cargo test -p gow-meet-core --lib pipeline::`
Expected: `test result: ok. 17 passed` (12 da etapa 1 + 5 novos).

- [ ] **Step 4: Teste de privacidade (logs e arquivos da sessão)**

`crates/core/Cargo.toml`, em `[dev-dependencies]`:
```toml
tracing-subscriber = { version = "0.3", default-features = false, features = ["fmt"] }
```

`crates/core/tests/voice_privacy.rs` (binário de teste próprio porque instala um coletor de logs global):
```rust
//! Impressões de voz e nomes nunca vão para os logs; impressões nunca vão para os
//! arquivos da sessão. Binário de teste próprio: instala um coletor de logs global.
use anyhow::Result;
use gow_meet_core::asr::Transcriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{Meta, SessionWriter, SpeakerSource};
use gow_meet_core::speaker::SpeakerEmbedder;
use gow_meet_core::tracker::{SpeakerTracker, TrackerConfig};
use gow_meet_core::translate::Translator;
use gow_meet_core::voices::{VoiceLibrary, VOICES_FILE};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

struct FakeAsr;
impl Transcriber for FakeAsr {
    fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok("Hello team.".into()) }
}
struct Prefix;
impl Translator for Prefix {
    fn translate(&mut self, t: &str) -> Result<String> { Ok(format!("pt: {t}")) }
}
/// Valores fáceis de procurar nos arquivos e logs.
struct Marked;
impl SpeakerEmbedder for Marked {
    fn embed(&mut self, _a: &[f32]) -> Result<Vec<f32>> { Ok(vec![0.123_456_7, 0.765_432_1, 0.0]) }
}
struct Chan(crossbeam_channel::Sender<PipelineEvent>);
impl Sink for Chan {
    fn emit(&self, ev: PipelineEvent) { let _ = self.0.send(ev); }
}

#[test]
fn embeddings_and_names_never_reach_logs_or_session_files() {
    let logs = Capture::default();
    let c = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt().with_ansi(false).with_max_level(tracing::Level::TRACE)
            .with_writer(move || c.clone()).finish()).unwrap();

    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);
    std::fs::write(&lib_path, "{ corrompido: Fulana Fictícia 0.1234567").unwrap();
    let (library, backup) = VoiceLibrary::load(&lib_path, 1);
    assert!(backup.is_some());
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "t".into(), title: "Teste".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let (tx, rx) = crossbeam_channel::unbounded();
    let sid = SpeakerId { embedder: Box::new(Marked),
                          tracker: SpeakerTracker::new(TrackerConfig::default(), "fake", library) };
    let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FakeAsr), Box::new(Prefix),
        Some(sid), SessionWriter::create(root.path(), meta).unwrap(), Arc::new(Chan(tx)));
    let speech: Vec<f32> = (0..1_500 * 16).map(|i| 0.3 * (i as f32 * 0.17).sin())
        .chain(std::iter::repeat(0.0).take(1_000 * 16)).collect();
    p.feed(SpeakerSource::System, &speech);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(rx.recv_timeout(deadline.saturating_duration_since(Instant::now())), Ok(PipelineEvent::Final(_))) {
        assert!(Instant::now() < deadline, "fala final não chegou");
    }
    p.rename_voice("v1", "Fulana Fictícia").unwrap();
    assert!(p.rename_voice("v1", "").is_err());
    p.feed(SpeakerSource::System, &speech);
    p.finish(10_000).unwrap();

    let log = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("voz nomeada"), "o teste precisa ver os logs: {log}");
    for secret in ["Fulana", "0.123", "0.765", "Hello team"] {
        assert!(!log.contains(secret), "log contém {secret:?}:\n{log}");
    }
    for entry in std::fs::read_dir(root.path().join("t")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in ["0.123", "0.765", "embedding"] {
            assert!(!text.contains(secret), "{} contém {secret:?}", path.display());
        }
    }
    // A impressão só existe no voices.json.
    assert!(std::fs::read_to_string(&lib_path).unwrap().contains("embedding"));
}
```

Run: `cargo test -p gow-meet-core --test voice_privacy`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 5: Reunião com vozes do `say` pelo pipeline**

Em `crates/core/tests/speaker_say.rs`, troque os `use` do topo por:
```rust
use anyhow::Result;
use gow_meet_core::asr::Transcriber;
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{Meta, SessionWriter, SpeakerSource};
use gow_meet_core::speaker::{cosine, normalize, SpeakerEmbedder, TractEmbedder};
use gow_meet_core::tracker::{SpeakerTracker, TrackerConfig};
use gow_meet_core::translate::Translator;
use gow_meet_core::voices::{VoiceLibrary, VOICES_FILE};
use gow_meet_core::wav::read_wav_16k_mono;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
```
e acrescente no fim do arquivo:
```rust
struct FixedAsr;
impl Transcriber for FixedAsr {
    fn transcribe(&mut self, _a: &[f32]) -> Result<String> { Ok("Sentence.".into()) }
}
struct Echo;
impl Translator for Echo {
    fn translate(&mut self, t: &str) -> Result<String> { Ok(t.into()) }
}
#[derive(Default)]
struct Finals(Mutex<Vec<(String, Option<String>)>>);
impl Sink for Finals {
    fn emit(&self, ev: PipelineEvent) {
        if let PipelineEvent::Final(l) = ev { self.0.lock().unwrap().push((l.speaker.label, l.speaker.voice)); }
    }
}

/// Uma reunião pelo pipeline: falas alternadas com 1,5 s de silêncio, em pedaços de 100 ms.
/// Com `names`, dá a cada grupo o nome da voz que mais apareceu nele antes de encerrar.
fn run_meeting(model: &Path, lib_path: &Path, order: &[(String, &Vec<f32>)], names: bool) -> Vec<(String, Option<String>)> {
    let (library, _) = VoiceLibrary::load(lib_path, 1);
    let sid = SpeakerId { embedder: Box::new(TractEmbedder::new(model).unwrap()),
                          tracker: SpeakerTracker::new(TrackerConfig::default(), "teste", library) };
    let root = tempfile::tempdir().unwrap();
    let meta = Meta { v: 1, id: "say".into(), title: "say".into(), started: 0, ended: None,
        source_lang: "en".into(), target_lang: "pt".into(), models: Default::default(), app_version: "0".into() };
    let sink = Arc::new(Finals::default());
    let p = Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(FixedAsr), Box::new(Echo),
        Some(sid), SessionWriter::create(root.path(), meta).unwrap(), sink.clone());
    let gap = vec![0.0f32; 1_500 * 16];
    for (_, audio) in order {
        for c in audio.chunks(1_600).chain(gap.chunks(1_600)) { p.feed(SpeakerSource::System, c); }
    }
    if names {
        let deadline = Instant::now() + std::time::Duration::from_secs(60);
        while sink.0.lock().unwrap().len() < order.len() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let got = sink.0.lock().unwrap().clone();
        let mut by_group: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for ((voice, _), (_, g)) in order.iter().zip(&got) {
            *by_group.entry(g.clone().unwrap_or_default()).or_default().entry(voice.clone()).or_default() += 1;
        }
        for (g, counts) in by_group {
            let name = counts.into_iter().max_by_key(|(_, n)| *n).unwrap().0;
            p.rename_voice(&g, &name).unwrap();
        }
    }
    p.finish(1).unwrap();
    let got = sink.0.lock().unwrap().clone();
    got
}

#[test]
#[ignore]
fn say_meeting_through_the_pipeline() {
    let model = model();
    let f = fixtures();
    let lib_dir = tempfile::tempdir().unwrap();
    let lib_path = lib_dir.path().join(VOICES_FILE);

    let first = meeting(&f, 0..4);
    let got = run_meeting(&model, &lib_path, &first, true);
    assert_eq!(got.len(), first.len(), "falas fechadas ≠ falas geradas");
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ((voice, _), (_, g)) in first.iter().zip(&got) { groups.entry(g.clone().unwrap()).or_default().insert(voice.clone()); }
    eprintln!("pipeline, reunião 1: {groups:?}");
    let n_voices = f.keys().map(|(v, _)| v).collect::<BTreeSet<_>>().len();
    assert_eq!(groups.len(), n_voices);
    assert!(groups.values().all(|s| s.len() == 1));

    let second = meeting(&f, 4..8);
    let got = run_meeting(&model, &lib_path, &second, false);
    let wrong = second.iter().zip(&got).filter(|((voice, _), (label, _))| voice != label).count();
    eprintln!("pipeline, reunião 2: {wrong}/{} erradas", second.len());
    assert_eq!(wrong, 0);
}
```

Run:
```bash
GOW_MEET_SPEAKER_MODEL=$PWD/target/voice-models/voxceleb_ECAPA512_LM.onnx \
GOW_MEET_VOICES=$PWD/target/voice-fixtures \
  cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
```
Expected: `pipeline, reunião 1: {"v1": {"Daniel"}, "v2": {"Karen"}, "v3": {"Moira"}, "v4": {"Samantha"}}`, `pipeline, reunião 2: 0/16 erradas`, `test result: ok. 3 passed`.

- [ ] **Step 6: Evento `speaker` no app (teste falhando)**

Em `src-tauri/src/lib.rs`, no teste `events_serialize_with_kind_tag`, depois da linha `assert_eq!(v["kind"], "reconnecting");`:
```rust
        let v = serde_json::to_value(UiEvent::Speaker { voice: "v2".into(), label: "Ana".into() }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "speaker", "voice": "v2", "label": "Ana"}));
        let line = Line { id: "s1".into(), t0: 0, t1: 900, speaker: Speaker::tracked("v2", "Participante 2"),
            text: "Hi.".into(), translation: None, translation_error: false };
        let v = serde_json::to_value(UiEvent::Final { line }).unwrap();
        assert_eq!(v["line"]["speaker"], serde_json::json!({"source": "system", "label": "Participante 2", "voice": "v2"}));
        let mic = Line { id: "m0".into(), t0: 0, t1: 1, speaker: Speaker::mic(), text: "Oi.".into(),
            translation: None, translation_error: false };
        let v = serde_json::to_value(UiEvent::Final { line: mic }).unwrap();
        assert!(v["line"]["speaker"].get("voice").is_none());
```

Run: `cargo test -p gow-meet-app events_serialize_with_kind_tag`
Expected: erro de compilação (`no variant named Speaker` em `UiEvent` e `non-exhaustive patterns: PipelineEvent::Speaker` no `TauriSink`).

- [ ] **Step 7: `UiEvent::Speaker`**

Em `src-tauri/src/lib.rs`, no `enum UiEvent`, depois de `Stopped { transcript: String },`:
```rust
    /// Grupo de voz renomeado: as duas janelas trocam o rótulo de todas as falas dele.
    Speaker { voice: String, label: String },
```
No `match` de `TauriSink::emit`, depois de `PipelineEvent::Dropped { id } => UiEvent::Dropped { id },`:
```rust
            PipelineEvent::Speaker { voice, label } => UiEvent::Speaker { voice, label },
```

Run: `cargo test -p gow-meet-app events_serialize_with_kind_tag`
Expected: `test result: ok. 1 passed`.

- [ ] **Step 8: Workspace inteiro e commit**

Run: `cargo test --workspace`
Expected: tudo passa (o app ainda chama `Pipeline::start`, que não mudou de assinatura; a identificação é ligada na Task 6).

```bash
git add crates/core/src/pipeline.rs crates/core/Cargo.toml Cargo.lock crates/cli/src/main.rs \
  crates/core/tests/voice_privacy.rs crates/core/tests/speaker_say.rs src-tauri/src/lib.rs
git commit -m "Identifica quem fala nas falas finais do sistema no pipeline" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 6: Comandos, modelo de voz e configuração no app Tauri

**Files:**
- Modify: `src-tauri/src/config.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, `src-tauri/capabilities/main.json`, `src-tauri/capabilities/overlay.json`, `src-tauri/Cargo.toml`, `README.md`
- Create (gerados pelo build): `src-tauri/permissions/autogenerated/{rename_voice,list_voices,rename_saved_voice,delete_voice,delete_all_voices}.toml`

**Interfaces:**
- Consumes: `TractEmbedder::new` e `DEFAULT_SPEAKER_MODEL` (Task 2); `VoiceLibrary` (`load`, `read_summaries`, `rename`, `delete`, `delete_all`, `save`, `len`), `VoiceSummary`, `VOICES_FILE`, `SpeakerTracker::new`, `TrackerConfig` (Task 3); `SpeakerId`, `Pipeline::start_with_speakers`, `Pipeline::rename_voice` (Task 5); `UiEvent::Speaker` (Task 5); `local_root(app)`, `ensure_one`, `Slot`, `AppState`, `lock` (etapa 1, `src-tauri/src/lib.rs`).
- Produces:
  - `config.json`: `speaker_model: String` (padrão `"wespeaker-ecapa512-lm"`), `speaker_library_threshold: f32` (0.70), `speaker_group_threshold: f32` (0.65), `speaker_min_ms: u64` (1000); `Config::tracker(&self) -> TrackerConfig`.
  - Comandos Tauri (nomes e argumentos exatos que o front-end usa na Task 7):
    - `rename_voice { voice: string, name: string } -> string` (rótulo novo; erro sem reunião, voz inexistente ou nome vazio);
    - `list_voices -> VoiceSummary[]` (`{id, name, updated, count}`, mais recentes primeiro; erro se `voices.json` está corrompido);
    - `rename_saved_voice { id: string, name: string } -> void`;
    - `delete_voice { id: string } -> void`;
    - `delete_all_voices -> void`;
    - os três últimos recusam durante a reunião com "Encerre a reunião para alterar as vozes guardadas.".
  - `voices.json` em `local_root(app)/voices.json`.
  - Avisos (`UiEvent::Error`, uma vez por reunião): `SPEAKER_UNAVAILABLE`, `VOICES_CORRUPT`.
  - `meta.json` ganha `models.speaker = <speaker_model>` quando a identificação está ligada.

- [ ] **Step 1: Teste da configuração (falhando)**

Em `src-tauri/src/config.rs`, no módulo `tests`, antes de `fn invalid_file_falls_back_to_defaults`:
```rust
    #[test]
    fn speaker_defaults_and_override() {
        let d = tempfile::tempdir().unwrap();
        let c = Config::load(&d.path().join("config.json"));
        assert_eq!(c.speaker_model, "wespeaker-ecapa512-lm");
        assert_eq!(c.tracker(), gow_meet_core::tracker::TrackerConfig::default());
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"speaker_group_threshold":0.6,"speaker_min_ms":1500}"#).unwrap();
        let c = Config::load(&p);
        assert_eq!((c.speaker_group_threshold, c.speaker_min_ms, c.speaker_library_threshold), (0.6, 1_500, 0.70));
    }

```

Run: `cargo test -p gow-meet-app config::`
Expected: erro de compilação (`no field speaker_model`, `no method named tracker`).

- [ ] **Step 2: Chaves novas do `config.json`**

Em `src-tauri/src/config.rs`, na `struct Config`, depois de `pub models_base_url: String,`:
```rust
    /// Modelo de impressão de voz (id do manifesto).
    pub speaker_model: String,
    /// Semelhança mínima (cosseno) para reconhecer uma voz guardada.
    pub speaker_library_threshold: f32,
    /// Semelhança mínima para juntar a fala a um grupo da reunião.
    pub speaker_group_threshold: f32,
    /// Falas mais curtas herdam o último falante do sistema.
    pub speaker_min_ms: u64,
```
No `Default`, depois de `models_base_url: gow_meet_core::models::DEFAULT_BASE_URL.into(),`:
```rust
            speaker_model: gow_meet_core::speaker::DEFAULT_SPEAKER_MODEL.into(),
            speaker_library_threshold: 0.70,
            speaker_group_threshold: 0.65,
            speaker_min_ms: 1_000,
```
No começo do `impl Config {`:
```rust
    pub fn tracker(&self) -> gow_meet_core::tracker::TrackerConfig {
        gow_meet_core::tracker::TrackerConfig {
            library_threshold: self.speaker_library_threshold,
            group_threshold: self.speaker_group_threshold,
            min_speech_ms: self.speaker_min_ms,
        }
    }

```

Run: `cargo test -p gow-meet-app config::`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 3: Teste da trava das vozes guardadas (falhando)**

Em `src-tauri/src/lib.rs`, no módulo `tests`, troque `use super::{read_sessions, Throttle, UiEvent};` por:
```rust
    use super::{read_sessions, voices_locked, Slot, Throttle, UiEvent};
```
e acrescente antes de `fn meta(id: &str, started: u64, ended: Option<u64>)`:
```rust
    #[test]
    fn saved_voices_only_change_outside_a_meeting() {
        assert_eq!(voices_locked(&Slot::Idle), None);
        assert!(voices_locked(&Slot::Starting).is_some());
        assert!(voices_locked(&Slot::Stopping).is_some());
    }

```

Run: `cargo test -p gow-meet-app saved_voices_only_change`
Expected: erro de compilação (`unresolved import super::voices_locked`).

- [ ] **Step 4: Modelo de voz opcional e identificação ligada na sessão**

`src-tauri/Cargo.toml`, em `[dependencies]`, depois de `serde_json = { workspace = true }`:
```toml
anyhow = { workspace = true }
```

Em `src-tauri/src/lib.rs`:

Troque as três linhas seguidas `use gow_meet_core::pipeline::…;`, `use gow_meet_core::session::…;` e `use gow_meet_core::translate::Ct2Translator;` por:
```rust
use gow_meet_core::pipeline::{Pipeline, PipelineConfig, PipelineEvent, Sink, SpeakerId};
use gow_meet_core::session::{recover_unfinished, session_id, Line, Meta, SessionWriter, Speaker, SpeakerSource};
use gow_meet_core::speaker::TractEmbedder;
use gow_meet_core::tracker::SpeakerTracker;
use gow_meet_core::translate::Ct2Translator;
use gow_meet_core::voices::{VoiceLibrary, VoiceSummary, VOICES_FILE};
```
Na `struct ModelPaths`, depois de `opus_dir: PathBuf,`:
```rust
    /// Arquivo do modelo de voz; `None` se não foi possível baixá-lo (a reunião segue sem
    /// identificar quem fala, e a próxima reunião tenta de novo).
    speaker: Option<PathBuf>,
```

Depois de `fn local_root(…) { … }`:
```rust
/// Vozes guardadas: pasta local, como os modelos (fora do perfil Roaming no Windows).
fn voices_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_root(app)?.join(VOICES_FILE))
}

fn now_ms() -> u64 { chrono::Local::now().timestamp_millis() as u64 }

```

Troque `fn ensure_all(…) { … }` inteiro (com o comentário acima dele) por:
```rust
/// Verifica/baixa os modelos uma vez por execução do app. O modelo de voz é opcional:
/// sem ele a reunião segue como na etapa 1.
fn ensure_all(app: &AppHandle, st: &AppState) -> Result<ModelPaths, String> {
    let mut cached = lock(&st.models);
    let mut paths = match cached.clone() {
        Some(p) => p,
        None => {
            let whisper_dir = ensure_one(app, &st.config, &st.config.whisper_model)?;
            let spec = models::manifest().remove(&st.config.whisper_model)
                .ok_or(format!("modelo {} fora do manifesto", st.config.whisper_model))?;
            let opus_dir = ensure_one(app, &st.config, &st.config.opus_model)?;
            ModelPaths { whisper: whisper_dir.join(&spec.files[0].name), opus_dir, speaker: None }
        }
    };
    if paths.speaker.is_none() { paths.speaker = ensure_speaker(app, &st.config); }
    *cached = Some(paths.clone());
    Ok(paths)
}

fn ensure_speaker(app: &AppHandle, cfg: &Config) -> Option<PathBuf> {
    let Some(name) = models::manifest().get(&cfg.speaker_model).and_then(|s| s.files.first()).map(|f| f.name.clone()) else {
        tracing::warn!(model = %cfg.speaker_model, "modelo de voz fora do manifesto");
        return None;
    };
    ensure_one(app, cfg, &cfg.speaker_model).ok().map(|dir| dir.join(name))
}

pub const SPEAKER_UNAVAILABLE: &str =
    "Identificação de quem fala indisponível nesta reunião: os participantes aparecem como \"Participante\".";
pub const VOICES_CORRUPT: &str =
    "O arquivo de vozes guardadas estava corrompido: ele foi preservado e a lista de vozes recomeça vazia.";

/// Identificação de quem fala. Sem modelo, ou se ele não carrega, avisa uma vez e a
/// reunião segue sem ela.
fn speaker_setup(app: &AppHandle, cfg: &Config, model: Option<&Path>) -> Option<SpeakerId> {
    let warn = |message: &str| { let _ = app.emit("gow-meet", UiEvent::Error { message: message.into() }); };
    let embedder = match model.map(TractEmbedder::new) {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            tracing::warn!("modelo de voz não carregou: {e:#}");
            warn(SPEAKER_UNAVAILABLE);
            return None;
        }
        None => {
            warn(SPEAKER_UNAVAILABLE);
            return None;
        }
    };
    let path = voices_path(app).ok()?;
    let (library, corrupt) = VoiceLibrary::load(&path, now_ms());
    if corrupt.is_some() { warn(VOICES_CORRUPT); }
    tracing::info!(vozes = library.len(), "identificação de vozes ligada");
    Some(SpeakerId { embedder: Box::new(embedder),
                     tracker: SpeakerTracker::new(cfg.tracker(), &cfg.speaker_model, library) })
}
```

Em `build_session`, troque a linha `let now = chrono::Local::now();` por:
```rust
    let speakers = speaker_setup(app, cfg, paths.speaker.as_deref());
    let mut model_ids = vec![("asr".to_string(), cfg.whisper_model.clone()), ("translate".to_string(), cfg.opus_model.clone())];
    if speakers.is_some() { model_ids.push(("speaker".to_string(), cfg.speaker_model.clone())); }
    let now = chrono::Local::now();
```
no `Meta { … }`, troque a linha `models: [("asr"…), ("translate"…)].into(),` por:
```rust
        models: model_ids.into_iter().collect(),
```
e troque a criação do pipeline por:
```rust
    let pipeline = Arc::new(Pipeline::start_with_speakers(PipelineConfig::default(), Box::new(asr), Box::new(tr),
                                                          speakers, writer, Arc::new(TauriSink(app.clone()))));
```

- [ ] **Step 5: Os cinco comandos**

Em `src-tauri/src/lib.rs`, antes de `fn init_logs(`:
```rust
/// Dá nome a um grupo de voz durante a reunião (janela flutuante). O slot fica travado
/// durante a renomeação: o encerramento espera e o pipeline não muda de dono no meio.
#[tauri::command]
async fn rename_voice(app: AppHandle, voice: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        let slot = lock(&st.slot);
        let Slot::Running(run) = &*slot else { return Err("nenhuma reunião em andamento".to_string()) };
        run.pipeline.rename_voice(&voice, &name).map_err(|e| format!("{e:#}"))
    }).await.map_err(|e| e.to_string())?
}

/// Vozes guardadas, sem impressão de voz. Só leitura: um arquivo corrompido vira erro
/// aqui e é preservado na próxima reunião ou alteração.
#[tauri::command]
fn list_voices(app: AppHandle) -> Result<Vec<VoiceSummary>, String> {
    VoiceLibrary::read_summaries(&voices_path(&app)?).map_err(|e| format!("{e:#}"))
}

/// Alterações nas vozes guardadas só fora de reunião: durante a reunião o rastreador tem a
/// biblioteca em memória e a regravaria ao encerrar (uma voz apagada voltaria).
fn voices_locked(slot: &Slot) -> Option<&'static str> {
    (!matches!(slot, Slot::Idle)).then_some("Encerre a reunião para alterar as vozes guardadas.")
}

/// O slot fica travado durante a alteração: nenhuma reunião começa no meio dela.
fn edit_voices(app: &AppHandle, f: impl FnOnce(&mut VoiceLibrary) -> anyhow::Result<()>) -> Result<(), String> {
    let st = app.state::<AppState>();
    let slot = lock(&st.slot);
    if let Some(msg) = voices_locked(&slot) { return Err(msg.into()); }
    let (mut library, _) = VoiceLibrary::load(&voices_path(app)?, now_ms());
    f(&mut library).and_then(|()| library.save()).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn rename_saved_voice(app: AppHandle, id: String, name: String) -> Result<(), String> {
    edit_voices(&app, |l| l.rename(&id, &name))
}

#[tauri::command]
fn delete_voice(app: AppHandle, id: String) -> Result<(), String> {
    edit_voices(&app, |l| l.delete(&id))?;
    tracing::info!(id = %id, "voz guardada apagada");
    Ok(())
}

#[tauri::command]
fn delete_all_voices(app: AppHandle) -> Result<(), String> {
    edit_voices(&app, |l| l.delete_all())?;
    tracing::info!("todas as vozes guardadas apagadas");
    Ok(())
}

```

Troque a linha do `invoke_handler` por:
```rust
        .invoke_handler(tauri::generate_handler![
            ensure_models, start_session, stop_session, list_sessions,
            rename_voice, list_voices, rename_saved_voice, delete_voice, delete_all_voices
        ])
```

- [ ] **Step 6: Permissões**

`src-tauri/build.rs`, troque o comentário e o `.commands(…)` por:
```rust
  // Declarar os comandos faz o Tauri exigir permissão para eles: só a janela
  // principal (capabilities/main.json) pode chamá-los; a flutuante, só `rename_voice`.
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
    tauri_build::AppManifest::new()
      .commands(&[
        "ensure_models", "start_session", "stop_session", "list_sessions",
        "rename_voice", "list_voices", "rename_saved_voice", "delete_voice", "delete_all_voices",
      ]),
  ))
```

`src-tauri/capabilities/main.json`: troque a `description` por `"janela principal: comandos (inclusive vozes guardadas), eventos e abertura da transcrição e dos Ajustes do Sistema"` e, depois de `"allow-list-sessions",`, acrescente:
```json
    "allow-rename-voice",
    "allow-list-voices",
    "allow-rename-saved-voice",
    "allow-delete-voice",
    "allow-delete-all-voices",
```

`src-tauri/capabilities/overlay.json` inteiro:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "overlay",
  "description": "janela flutuante: ouvir eventos, ser arrastada e dar nome a uma voz durante a reunião",
  "windows": ["overlay"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "allow-rename-voice"
  ]
}
```

Run:
```bash
export CMAKE_POLICY_VERSION_MINIMUM=3.5
cargo test -p gow-meet-app
ls src-tauri/permissions/autogenerated/
```
Expected: todos os testes do app passam (inclusive `saved_voices_only_change_outside_a_meeting` e `speaker_defaults_and_override`); a pasta tem 9 arquivos `.toml`, entre eles `rename_voice.toml` com `identifier = "allow-rename-voice"`.

- [ ] **Step 7: README**

Em `README.md`:

No parágrafo de abertura, depois da frase que termina em "e a transcrição fica salva em Markdown ao encerrar.", acrescente:
```markdown
Desde a etapa 2, as falas do áudio do sistema são separadas por voz (**Participante 1**,
**Participante 2**…); clique no rótulo na janela flutuante para dar nome a uma voz. O app
lembra das vozes nomeadas nas próximas reuniões, neste computador.
```

Na tabela de "Onde ficam os dados", acrescente as linhas:
```markdown
| Nomes das vozes de cada reunião (`speakers.json`) | `<dados>/sessions/<id>/` |
| Vozes guardadas (`voices.json`) | `<locais>/voices.json` |
```

Na tabela de `config.json`, acrescente:
```markdown
| `speaker_model` | `"wespeaker-ecapa512-lm"` | Modelo de impressão de voz (quem falou) |
| `speaker_library_threshold` | `0.70` | Semelhança mínima para reconhecer uma voz guardada |
| `speaker_group_threshold` | `0.65` | Semelhança mínima para juntar a fala a uma voz da reunião |
| `speaker_min_ms` | `1000` | Falas mais curtas herdam o último participante que falou |
```

Depois do parágrafo sobre `GOW_MEET_MODELS_URL` (fim da seção `### config.json`) e antes de `## Modelos e licenças` (Task 1), acrescente:
```markdown
## Vozes guardadas e privacidade

Ao dar nome a uma voz, o app guarda uma **impressão de voz** (um vetor numérico; não é
áudio) em `voices.json`, só neste computador, na pasta local (fora do perfil que
sincroniza). Impressão de voz é dado biométrico (dado pessoal sensível na LGPD): o
consentimento das pessoas é responsabilidade da gow, por processos próprios, fora do app.

- A impressão nunca vai para os arquivos da reunião, para os logs ou para a tela.
- Na janela principal, **Vozes guardadas** lista nome, última vez vista e número de falas,
  e permite renomear, apagar uma voz ou apagar todas (fora de reunião).
- "Apagar todas" também apaga cópias preservadas de um `voices.json` corrompido.
- Apagar uma voz não muda transcrições antigas: o nome fica lá como texto.
```

- [ ] **Step 8: Conferência rápida no app**

Run:
```bash
npm run build && export CMAKE_POLICY_VERSION_MINIMUM=3.5 && npx tauri dev
```
Na janela principal: **Iniciar**. Expected: a lista de downloads mostra `voxceleb_ECAPA512_LM.onnx` (~25 MB) uma vez; com áudio de um vídeo em inglês tocando, a flutuante mostra falas com "PARTICIPANTE 1" (o rótulo clicável chega na Task 7). **Encerrar**. Conferir:
```bash
grep -h "identificação de vozes" ~/Library/Application\ Support/gow-meet/logs/gow-meet.*.log | tail -2
cat "$(ls -dt ~/Library/Application\ Support/gow-meet/sessions/*/ | head -1)speakers.json"
ls ~/Library/Application\ Support/gow-meet/voices.json 2>/dev/null || echo "sem voices.json (nenhuma voz nomeada)"
```
Expected: `identificação de vozes ligada vozes=0` e `identificação de vozes encerrada grupos=N …`; `speakers.json` com `"v1": {"label": "Participante 1"}`…; ainda sem `voices.json`.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/config.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/Cargo.toml Cargo.lock \
  src-tauri/capabilities/main.json src-tauri/capabilities/overlay.json \
  src-tauri/permissions/autogenerated/*.toml README.md
git commit -m "Liga a identificação de vozes no app e adiciona os comandos de vozes" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 7: Interface — renomear na janela flutuante e seção "Vozes guardadas"

**Files:**
- Create: `src/lib/rename.ts`, `src/lib/rename.test.ts`, `src/SavedVoices.tsx`
- Modify: `src/lib/transcript.ts`, `src/lib/transcript.test.ts`, `src/Overlay.tsx`, `src/MainWindow.tsx`, `src/icons.tsx`, `src/styles.css`, `src/demo/Demo.tsx`

**Interfaces:**
- Consumes: comandos `rename_voice {voice, name}`, `list_voices`, `rename_saved_voice {id, name}`, `delete_voice {id}`, `delete_all_voices` (Task 6); evento `"gow-meet"` com `{kind: "speaker", voice, label}` e `line.speaker.voice` (Task 5).
- Produces:
  - TS: `Speaker = { source: "mic" | "system"; label: string; voice?: string }`; `UiEvent` ganha `{ kind: "speaker"; voice: string; label: string }`; `State.labels: Record<string, string>`.
  - `src/lib/rename.ts`: `nameKey(key: string, draft: string): NameKeyAction` (`{action:"save", name}` | `{action:"cancel"}` | `{action:"none"}`), `MAX_NAME = 60`.
  - `src/SavedVoices.tsx`: `SavedVoice = { id; name; updated; count }`, `VoicesUi`, `SavedVoices` (componente), `PRIVACY_TEXT`.
  - `OverlayView({ state, onRename?, editing? })`; `MainViewProps` ganha `voices`, `voicesError`, `onRenameVoice`, `onDeleteVoice`, `onDeleteAllVoices`, `voicesUi?`.
  - Estados novos do modo demo: `overlay-speakers`, `overlay-rename`, `main-voices`, `main-voices-rename`, `main-voices-confirm`, `main-voices-confirm-all`, `main-voices-error`.

- [ ] **Step 1: Testes (falhando)**

`src/lib/rename.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import { nameKey } from "./rename";

describe("nameKey", () => {
  it("Enter salva o nome aparado", () => {
    expect(nameKey("Enter", "  Ana  ")).toEqual({ action: "save", name: "Ana" });
  });
  it("Esc cancela, mesmo com texto", () => {
    expect(nameKey("Escape", "Ana")).toEqual({ action: "cancel" });
  });
  it("Enter com nome vazio cancela", () => {
    expect(nameKey("Enter", "   ")).toEqual({ action: "cancel" });
    expect(nameKey("Enter", "")).toEqual({ action: "cancel" });
  });
  it("outras teclas seguem editando", () => {
    expect(nameKey("a", "An")).toEqual({ action: "none" });
    expect(nameKey("Tab", "Ana")).toEqual({ action: "none" });
  });
});
```

Em `src/lib/transcript.test.ts`, antes de `it("classifica avisos de permissão", …)`:
```ts
  it("evento speaker troca o rótulo de todas as falas do grupo", () => {
    const v1 = { source: "system" as const, label: "Participante 1", voice: "v1" };
    const v2 = { source: "system" as const, label: "Participante 2", voice: "v2" };
    const me = { source: "mic" as const, label: "Você" };
    const s = run(
      { kind: "final", line: { ...line("s0", 0, "Hello.", null), speaker: v1 } },
      { kind: "final", line: { ...line("s1", 1000, "Hi.", null), speaker: v2 } },
      { kind: "final", line: { ...line("m0", 1500, "Hey.", null), speaker: me } },
      { kind: "final", line: { ...line("s2", 2000, "Next topic.", null), speaker: v1 } },
      { kind: "speaker", voice: "v1", label: "Ana" },
    );
    expect(s.order.map((id) => s.byId[id].speaker.label)).toEqual(["Ana", "Participante 2", "Você", "Ana"]);
    expect(s.byId.s0.speaker).toEqual({ source: "system", label: "Ana", voice: "v1" });
  });

  it("tradução que chega depois do nome mantém o nome", () => {
    const v1 = { source: "system" as const, label: "Participante 1", voice: "v1" };
    const s = run(
      { kind: "final", line: { ...line("s0", 0, "Hello.", null), speaker: v1 } },
      { kind: "speaker", voice: "v1", label: "Ana" },
      { kind: "translated", line: { ...line("s0", 0, "Hello.", "Olá."), speaker: v1 } },
      { kind: "final", line: { ...line("s1", 900, "Bye.", null), speaker: v1 } },
    );
    expect(s.byId.s0).toMatchObject({ translation: "Olá.", speaker: { label: "Ana" } });
    expect(s.byId.s1.speaker.label).toBe("Ana");
  });

  it("reset esquece os nomes da reunião anterior", () => {
    const s = run({ kind: "speaker", voice: "v1", label: "Ana" }, { kind: "reset" });
    expect(s.labels).toEqual({});
  });

```

Run: `npm test`
Expected: FAIL (`Failed to resolve import "./rename"`; erros de tipo/asserção nos testes de `speaker`).

- [ ] **Step 2: Estado e teclas do campo de nome**

`src/lib/rename.ts`:
```ts
/** Teclas do campo de nome (janela flutuante e vozes guardadas): Enter salva, Esc cancela,
 *  nome vazio cancela. */
export type NameKeyAction = { action: "save"; name: string } | { action: "cancel" } | { action: "none" };

export const MAX_NAME = 60;

export function nameKey(key: string, draft: string): NameKeyAction {
  if (key === "Escape") return { action: "cancel" };
  if (key !== "Enter") return { action: "none" };
  const name = draft.trim();
  return name ? { action: "save", name } : { action: "cancel" };
}
```

`src/lib/transcript.ts`:

Troque a primeira linha (`export type Speaker = …`) por:
```ts
/** `voice`: grupo de voz na reunião ("v1"…), só em falas do sistema identificadas. */
export type Speaker = { source: "mic" | "system"; label: string; voice?: string };
```
No tipo `UiEvent`, troque `| { kind: "stopped"; transcript: string };` por:
```ts
  | { kind: "stopped"; transcript: string }
  /** Grupo de voz renomeado: todas as falas com `speaker.voice === voice` mostram `label`. */
  | { kind: "speaker"; voice: string; label: string };
```
No tipo `State`, depois de `permissions: Partial<Record<PermissionKind, string>>;`:
```ts
  /** Nomes dados nesta reunião (evento `speaker`): valem também para falas e traduções
   *  que chegam depois com o rótulo antigo. */
  labels: Record<string, string>;
```
Troque `export const initial: State = …;` por:
```ts
export const initial: State = {
  order: [], byId: {}, lagMs: null, notice: null, transcript: null, permissions: {}, labels: {},
};

/** Aplica o nome dado ao grupo de voz, se houver. */
const relabel = (sp: Speaker, labels: Record<string, string>): Speaker =>
  sp.voice !== undefined && labels[sp.voice] !== undefined ? { ...sp, label: labels[sp.voice] } : sp;
```
Troque as duas primeiras linhas de `const fromLine` por:
```ts
const fromLine = (l: Line, labels: Record<string, string>, prev?: Entry): Entry => ({
  id: l.id, speaker: relabel(l.speaker, labels), t0: l.t0, text: l.text, final: true,
```
Em `applyEvent`, troque os `return` de `"final"` e `"translated"` por:
```ts
      return { ...upsert(s, fromLine(ev.line, s.labels, s.byId[ev.line.id])), lagMs: null, notice: null };
    case "translated":
      return upsert(s, { ...fromLine(ev.line, s.labels, s.byId[ev.line.id]), translation: ev.line.translation ?? null });
```
e, depois do `case "stopped": …`, acrescente:
```ts
    case "speaker": {
      const labels = { ...s.labels, [ev.voice]: ev.label };
      const byId: Record<string, Entry> = {};
      for (const [id, e] of Object.entries(s.byId)) byId[id] = { ...e, speaker: relabel(e.speaker, labels) };
      return { ...s, labels, byId };
    }
```

Run: `npm test`
Expected: `Tests  20 passed (20)`.

- [ ] **Step 3: Rótulo clicável na janela flutuante**

`src/icons.tsx`, no fim:
```tsx

export const IconPencil = () => (
  <svg {...base} width={12} height={12}>
    <path d="M4 20h4L19 9l-4-4L4 16v4z" />
    <path d="M13.5 6.5l4 4" />
  </svg>
);
```

`src/Overlay.tsx` inteiro:
```tsx
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import symbol from "./assets/brand/gow-simbolo-negativo.png";
import { IconPencil } from "./icons";
import { MAX_NAME, nameKey } from "./lib/rename";
import { lastEntries, type Speaker, type State } from "./lib/transcript";
import { useGowMeet } from "./lib/useGowMeet";

type Rename = (voice: string, name: string) => Promise<unknown>;

export function Overlay() {
  return (
    <OverlayView state={useGowMeet()}
      onRename={(voice, name) => invoke<string>("rename_voice", { voice, name })} />
  );
}

/** Apresentação da janela flutuante (usada também no modo demo). O painel arrasta; só o
 *  rótulo de quem fala (sem `data-tauri-drag-region`) é clicável. `editing`: só para o demo. */
export function OverlayView({ state, onRename, editing }: { state: State; onRename?: Rename; editing?: string }) {
  const [renameError, setRenameError] = useState<string | null>(null);
  const entries = lastEntries(state, 3);
  const hasPermissionIssue = Object.keys(state.permissions).length > 0;
  const status = hasPermissionIssue
    ? "Falta permissão de áudio: veja a janela principal"
    : renameError ?? (state.lagMs ? `Atraso de ${Math.round(state.lagMs / 1000)} s` : state.notice);
  const rename = onRename && ((voice: string, name: string) => {
    setRenameError(null);
    return onRename(voice, name).catch((e) => setRenameError(`Nome não salvo: ${String(e)}`));
  });
  return (
    <div className="overlay" data-tauri-drag-region>
      <div className="overlay-head" data-tauri-drag-region>
        <img className="overlay-symbol" src={symbol} alt="" draggable={false} data-tauri-drag-region />
        {entries.length > 0 && (
          <span className="overlay-live" data-tauri-drag-region><span className="dot" />Ao vivo</span>
        )}
        {status && (
          <span className={`overlay-status ${!hasPermissionIssue && (renameError || state.lagMs) ? "lag" : ""}`}
            data-tauri-drag-region>
            {status}
          </span>
        )}
      </div>
      <div className="captions" data-tauri-drag-region>
        {entries.length === 0 && <p className="waiting" data-tauri-drag-region>Aguardando fala…</p>}
        {entries.map((e) => (
          <div key={e.id} className={`caption ${e.final ? "" : "partial"}`} data-tauri-drag-region>
            <SpeakerLabel speaker={e.speaker} onRename={rename}
              startEditing={editing !== undefined && editing === e.speaker.voice} />
            <div className="translation" data-tauri-drag-region>
              {e.translation ?? (e.translationError ? "(sem tradução)" : "…")}
            </div>
            <div className="original" data-tauri-drag-region>{e.text}</div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** "VOCÊ" em amarelo; participantes em cinza claro. Falas do sistema com grupo de voz
 *  viram botão: clique abre o campo no lugar do rótulo (Enter salva; Esc ou vazio cancela). */
function SpeakerLabel({ speaker, onRename, startEditing = false }:
  { speaker: Speaker; onRename?: Rename; startEditing?: boolean }) {
  const [editing, setEditing] = useState(startEditing);
  const [draft, setDraft] = useState("");
  if (speaker.source === "mic") return <div className="speaker me" data-tauri-drag-region>{speaker.label}</div>;
  const voice = speaker.voice;
  if (!voice || !onRename) return <div className="speaker" data-tauri-drag-region>{speaker.label}</div>;
  if (editing) {
    return (
      <input className="speaker-input" autoFocus value={draft} maxLength={MAX_NAME}
        placeholder={speaker.label} aria-label="Nome de quem fala"
        onChange={(ev) => setDraft(ev.target.value)}
        onBlur={() => setEditing(false)}
        onKeyDown={(ev) => {
          const r = nameKey(ev.key, draft);
          if (r.action === "none") return;
          ev.preventDefault();
          setEditing(false);
          if (r.action === "save") void onRename(voice, r.name);
        }} />
    );
  }
  return (
    <button type="button" className="speaker speaker-edit" title="Dar nome a esta voz"
      onClick={() => { setDraft(""); setEditing(true); }}>
      {speaker.label}<IconPencil />
    </button>
  );
}
```

`src/styles.css`: troque o comentário `/* Nada é clicável aqui: o alvo do mouse é sempre um elemento com data-tauri-drag-region. */` por `/* Tudo arrasta (data-tauri-drag-region), menos o rótulo de quem fala, que é clicável. */` e, depois da regra `.speaker.me { color: var(--amarelo); }`, acrescente:
```css
/* Rótulo clicável: mesma tipografia, sem cor por pessoa; o lápis só aparece no hover. */
.speaker-edit {
  display: inline-flex; align-items: center; gap: 6px; padding: 0; background: none; border: 0;
  cursor: pointer; text-align: left;
}
.speaker-edit svg { opacity: 0; transition: opacity 0.12s; color: var(--texto-2); }
.speaker-edit:hover svg, .speaker-edit:focus-visible svg { opacity: 1; }
.speaker-edit:hover { color: var(--branco); }
.speaker-edit:focus-visible { outline: 1px solid var(--amarelo); outline-offset: 2px; }
.speaker-input {
  display: block; width: 220px; max-width: 100%; height: 22px; margin-bottom: 2px; padding: 0 6px;
  border-radius: 3px; background: var(--superficie); border: 1px solid var(--amarelo); color: var(--branco);
  font-family: var(--titulo); font-weight: 700; font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase;
  user-select: text; -webkit-user-select: text;
}
.speaker-input:focus { outline: none; }
.speaker-input::placeholder { color: var(--cinza); }
```

- [ ] **Step 4: Seção "Vozes guardadas" na janela principal**

`src/SavedVoices.tsx`:
```tsx
import { useState } from "react";
import { formatDate } from "./lib/format";
import { MAX_NAME, nameKey } from "./lib/rename";

/** Item de `list_voices`: nunca traz a impressão de voz. */
export type SavedVoice = { id: string; name: string; updated: number; count: number };

/** Estado local da seção; o modo demo passa um inicial para mostrar cada situação. */
export type VoicesUi = { editing: string | null; draft: string; confirm: "all" | string | null };
const idle: VoicesUi = { editing: null, draft: "", confirm: null };

export type SavedVoicesProps = {
  voices: SavedVoice[];
  error: string | null;
  /** Durante a reunião as vozes guardadas ficam só para leitura. */
  locked: boolean;
  onRename: (id: string, name: string) => void;
  onDelete: (id: string) => void;
  onDeleteAll: () => void;
  initial?: VoicesUi;
};

export const PRIVACY_TEXT =
  "Vozes ficam só neste computador. Apagar remove a impressão de voz; o consentimento é responsabilidade da gow.";

export function SavedVoices(p: SavedVoicesProps) {
  const [ui, setUi] = useState<VoicesUi>(p.initial ?? idle);
  const reset = () => setUi(idle);
  return (
    <section className="section">
      <div className="section-head">
        <h2 className="title">Vozes guardadas</h2>
        {p.voices.length > 0 && (ui.confirm === "all"
          ? (
            <span className="confirm">
              <span>Apagar todas as vozes?</span>
              <button className="btn btn-danger btn-small" onClick={() => { reset(); p.onDeleteAll(); }}>Apagar todas</button>
              <button className="btn btn-ghost btn-small" onClick={reset}>Cancelar</button>
            </span>
          )
          : (
            <button className="btn btn-ghost btn-small" disabled={p.locked}
              onClick={() => setUi({ ...idle, confirm: "all" })}>Apagar todas</button>
          ))}
      </div>
      <p className="note privacy">{PRIVACY_TEXT}</p>
      {p.locked && p.voices.length > 0 && <p className="note">Encerre a reunião para alterar as vozes guardadas.</p>}
      {p.error && <p className="voices-error" role="status">{p.error}</p>}
      {p.voices.length === 0
        ? <p className="empty">Nenhuma voz guardada. Dê nome a uma voz na janela flutuante durante a reunião.</p>
        : (
          <ul className="voices">
            {p.voices.map((v) => (
              <li key={v.id} className="voice">
                {ui.editing === v.id
                  ? (
                    <input className="field field-small" autoFocus value={ui.draft} maxLength={MAX_NAME}
                      placeholder={v.name} aria-label={`Novo nome para ${v.name}`}
                      onChange={(e) => setUi({ ...ui, draft: e.target.value })}
                      onBlur={reset}
                      onKeyDown={(e) => {
                        const r = nameKey(e.key, ui.draft);
                        if (r.action === "none") return;
                        e.preventDefault();
                        reset();
                        if (r.action === "save") p.onRename(v.id, r.name);
                      }} />
                  )
                  : <span className="voice-name">{v.name}</span>}
                <span className="voice-seen">Vista em {formatDate(v.updated)}</span>
                <span className="voice-count">{v.count} {v.count === 1 ? "fala" : "falas"}</span>
                <span className="voice-actions">
                  {ui.confirm === v.id
                    ? (
                      <>
                        <button className="btn btn-danger btn-small" onClick={() => { reset(); p.onDelete(v.id); }}>Apagar</button>
                        <button className="btn btn-ghost btn-small" onClick={reset}>Cancelar</button>
                      </>
                    )
                    : (
                      <>
                        <button className="btn btn-ghost btn-small" disabled={p.locked}
                          onClick={() => setUi({ ...idle, editing: v.id })}>Renomear</button>
                        <button className="btn btn-ghost btn-small" disabled={p.locked}
                          onClick={() => setUi({ ...idle, confirm: v.id })}>Apagar</button>
                      </>
                    )}
                </span>
              </li>
            ))}
          </ul>
        )}
    </section>
  );
}
```

`src/MainWindow.tsx`:

Depois de `import { resetTranscript, useGowMeet } from "./lib/useGowMeet";`:
```tsx
import { SavedVoices, type SavedVoice, type VoicesUi } from "./SavedVoices";
```
Depois de `const mb = (n: number) => Math.round(n / 1e6);`:
```tsx

/** Relê as vozes guardadas; um erro (ex.: arquivo corrompido) aparece na seção. */
const loadVoices = (set: (v: SavedVoice[]) => void, setError: (e: string | null) => void) =>
  invoke<SavedVoice[]>("list_voices")
    .then((v) => { set(v); setError(null); })
    .catch((e) => setError(String(e)));
```
Em `MainWindow`, troque `const refresh = () => invoke<Session[]>("list_sessions").then(setSessions).catch(() => {});` por:
```tsx
  const [voices, setVoices] = useState<SavedVoice[]>([]);
  const [voicesError, setVoicesError] = useState<string | null>(null);

  const refresh = () => {
    invoke<Session[]>("list_sessions").then(setSessions).catch(() => {});
    loadVoices(setVoices, setVoicesError);
  };
  /** Ação nas vozes guardadas: o erro aparece na seção; a lista é relida sempre. */
  const voiceAction = (cmd: string, args?: Record<string, unknown>) =>
    invoke(cmd, args).catch((e) => setVoicesError(String(e)))
      .finally(() => loadVoices(setVoices, setVoicesError));
```
No `<MainView … />` de `MainWindow`, depois de `lastTranscript={state.transcript} sessions={sessions} onOpen={open}`:
```tsx
      voices={voices} voicesError={voicesError}
      onRenameVoice={(id, name) => voiceAction("rename_saved_voice", { id, name })}
      onDeleteVoice={(id) => voiceAction("delete_voice", { id })}
      onDeleteAllVoices={() => voiceAction("delete_all_voices")}
```
Em `MainViewProps`, depois de `onOpen: (path: string) => void;`:
```tsx
  voices: SavedVoice[];
  voicesError: string | null;
  onRenameVoice: (id: string, name: string) => void;
  onDeleteVoice: (id: string) => void;
  onDeleteAllVoices: () => void;
  /** Só para o modo demo: estado inicial da seção de vozes. */
  voicesUi?: VoicesUi;
```
Em `MainView`, depois do `</section>` da seção "Reuniões" (antes de `</main>`):
```tsx

          <SavedVoices voices={p.voices} error={p.voicesError} locked={phase !== "idle"}
            onRename={p.onRenameVoice} onDelete={p.onDeleteVoice} onDeleteAll={p.onDeleteAllVoices}
            initial={p.voicesUi} />
```

`src/styles.css`, depois da regra `.empty { … }`:
```css

.privacy { margin: 0 0 14px; }
.voices { list-style: none; padding: 0; margin: 0; border-top: 1px solid var(--borda); }
.voice {
  display: grid; grid-template-columns: minmax(0, 1fr) 150px 70px auto; gap: 16px; align-items: center;
  min-height: 52px; padding: 8px 4px; border-bottom: 1px solid var(--borda); font-size: 14px;
}
.voice-name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.voice-seen, .voice-count { color: var(--texto-2); font-size: 13px; font-variant-numeric: tabular-nums; }
.voice-actions { display: flex; gap: 8px; justify-content: flex-end; }
.field-small { height: 34px; font-size: 14px; padding: 0 10px; }
.btn-danger { background: transparent; color: var(--amarelo); border-color: var(--amarelo); }
.btn-danger:hover { background: var(--amarelo); color: var(--preto); }
.confirm { display: flex; align-items: center; gap: 10px; font-size: 13px; color: var(--texto-2); }
.voices-error { margin: 0 0 12px; color: var(--amarelo); font-size: 13px; }
```

- [ ] **Step 5: Estados novos do modo demo**

`src/demo/Demo.tsx`:

Depois de `import { applyEvent, initial, type State, type UiEvent } from "../lib/transcript";`:
```tsx
import type { SavedVoice } from "../SavedVoices";
```
Troque `const baseMain: MainViewProps = { … };` por:
```tsx
// Nomes fictícios.
const voices: SavedVoice[] = [
  { id: "a", name: "Ana", updated: now, count: 42 },
  { id: "b", name: "Bruno", updated: now - 26 * 60 * min, count: 17 },
  { id: "c", name: "Carla", updated: now - 3 * 24 * 60 * min, count: 1 },
];

const baseMain: MainViewProps = {
  phase: "idle", title: "", onTitle: noop, onStart: noop, onStop: noop, progress: [],
  error: null, notice: null, permissions: {}, onOpenSettings: noop, lastTranscript: null,
  sessions, onOpen: noop, voices: [], voicesError: null,
  onRenameVoice: noop, onDeleteVoice: noop, onDeleteAllVoices: noop,
};
```
Em `mainStates`, depois da linha `"main-idle": …,`:
```tsx
  "main-voices": { ...baseMain, voices },
  "main-voices-rename": { ...baseMain, voices, voicesUi: { editing: "b", draft: "Bruno S", confirm: null } },
  "main-voices-confirm": { ...baseMain, voices, voicesUi: { editing: null, draft: "", confirm: "c" } },
  "main-voices-confirm-all": { ...baseMain, voices, voicesUi: { editing: null, draft: "", confirm: "all" } },
  "main-voices-error": { ...baseMain, voicesError: "o arquivo de vozes guardadas está corrompido" },
```
Em `"main-running"`, depois da linha `permissions: { screen: … },`:
```tsx
    voices,
```
Troque `const overlayStates: Record<string, State> = { … };` por:
```tsx
// Etapa 2: grupos de voz, um deles já nomeado durante a reunião.
const speakersEv: UiEvent[] = [
  { kind: "final", line: { id: "s0", t0: 0, t1: 2800, speaker: { source: "system", label: "Participante 1", voice: "v1" },
    text: "Let's review the project timeline for next week.",
    translation: "Vamos revisar o cronograma do projeto para a próxima semana." } },
  { kind: "final", line: { id: "s1", t0: 3200, t1: 5400, speaker: { source: "system", label: "Participante 2", voice: "v2" },
    text: "The new dashboard should be ready by Friday.",
    translation: "O novo painel deve ficar pronto até sexta." } },
  { kind: "final", line: { id: "m0", t0: 5600, t1: 7000, speaker: { source: "mic", label: "Você" },
    text: "Sure, I can share the updated plan.",
    translation: "Claro, posso compartilhar o plano atualizado." } },
  { kind: "speaker", voice: "v1", label: "Ana" },
];
const overlayStates: Record<string, State> = {
  "overlay": ev.reduce(applyEvent, initial),
  "overlay-empty": initial,
  "overlay-speakers": speakersEv.reduce(applyEvent, initial),
  "overlay-rename": speakersEv.reduce(applyEvent, initial),
};
/** Estados da flutuante que abrem com o campo de nome aberto no grupo indicado. */
const overlayEditing: Record<string, string> = { "overlay-rename": "v2" };
```
Troque `<div style={{ width: 440, height: 320 }}><OverlayView state={overlayStates[name]} /></div>` por:
```tsx
        <div style={{ width: 440, height: 320 }}>
          <OverlayView state={overlayStates[name]} onRename={() => Promise.resolve()} editing={overlayEditing[name]} />
        </div>
```

- [ ] **Step 6: Tipos, lint, testes e build**

Run: `npx tsc -b && npm run lint && npm test && npm run build`
Expected: sem erros nem avisos do ESLint; `Tests  20 passed (20)`; build gerado em `dist/`.

- [ ] **Step 7: Conferir o visual no modo demo**

Run: `npx vite` e abra no navegador (fora do Tauri):
- `http://localhost:5173/?demo=overlay-speakers` — "ANA" e "PARTICIPANTE 2" em cinza claro, "VOCÊ" em amarelo, Rajdhani caixa alta; o lápis aparece só no hover do rótulo; nenhuma cor por pessoa.
- `?demo=overlay-rename` — campo no lugar de "PARTICIPANTE 2", borda amarela, texto em caixa alta.
- `?demo=main-voices` — seção "VOZES GUARDADAS" abaixo de "REUNIÕES", com o texto de privacidade literal, nome, "Vista em …", "N falas", Renomear/Apagar e "Apagar todas".
- `?demo=main-voices-rename`, `?demo=main-voices-confirm`, `?demo=main-voices-confirm-all`, `?demo=main-voices-error`, `?demo=main-running` (botões desabilitados e aviso "Encerre a reunião…").

Expected: tudo legível em 900×640; nada quebra linha de forma estranha na lista.

- [ ] **Step 8: Conferir no app**

Run: `export CMAKE_POLICY_VERSION_MINIMUM=3.5 && npx tauri dev`. Com dois vídeos em inglês com vozes diferentes tocando um depois do outro:
1. Clique em "PARTICIPANTE 1" na flutuante → campo; digite um nome fictício, Enter → todas as falas desse grupo mostram o nome; a janela continua arrastável pelo resto do painel.
2. Clique de novo e aperte Esc → nada muda; clique, apague tudo e Enter → nada muda.
3. Encerrar → `transcript.md` mostra o nome também nas falas anteriores à renomeação; a seção "Vozes guardadas" lista a voz com "1 fala" ou mais.
4. Renomear e Apagar (com confirmação) na lista; Apagar todas (com confirmação) → lista vazia e `ls ~/Library/Application\ Support/gow-meet/voices.json*` não mostra cópias.
5. Com o nome fictício que você usou no item 1 (ex.: `Fulana Teste`): `grep -ci "Fulana Teste" ~/Library/Application\ Support/gow-meet/logs/*.log` → `0` em todos.

- [ ] **Step 9: Commit**

```bash
git add src/lib/rename.ts src/lib/rename.test.ts src/lib/transcript.ts src/lib/transcript.test.ts \
  src/Overlay.tsx src/SavedVoices.tsx src/MainWindow.tsx src/icons.tsx src/styles.css src/demo/Demo.tsx
git commit -m "Adiciona o nome das vozes na janela flutuante e a seção de vozes guardadas" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

### Task 8: Validação manual no Mac e verificação no Windows

**Files:**
- Create: `docs/validacao-falantes.md`

**Interfaces:**
- Consumes: o app completo (Tasks 1–7); `docs/validacao-mac.md` (números de CPU e atraso da etapa 1, para comparar); testes ignorados de `crates/core/tests/speaker_say.rs`.
- Produces: `docs/validacao-falantes.md` só com números; ajustes de `speaker_*` padrão em `src-tauri/src/config.rs`, se a calibração pedir (com o teste `speaker_defaults_and_override` e o `README.md` atualizados juntos).

- [ ] **Step 1: Build assinado**

Run:
```bash
export CMAKE_POLICY_VERSION_MINIMUM=3.5
APPLE_SIGNING_IDENTITY="gow-meet dev" npx tauri build --bundles app
open target/release/bundle/macos/gow-meet.app
```
Expected: o app abre com as permissões já concedidas (mesma identidade da etapa 1).

- [ ] **Step 2: Reunião 1 — ≥ 3 participantes remotos, nomear as vozes**

Numa reunião real pelo Teams com pelo menos 3 participantes remotos (que sabem da transcrição e concordaram em ter a voz guardada, conforme o processo da gow): dê nome a cada voz na flutuante assim que ela aparecer. Logo depois de encerrar, ainda com a reunião na memória, revise as falas do sistema **localmente** (nada disto vai para o git):
```bash
D="$(ls -dt ~/Library/Application\ Support/gow-meet/sessions/*/ | head -1)"
python3 - "$D" <<'PY'
import json, sys, pathlib
d = pathlib.Path(sys.argv[1])
sp = json.loads((d / "speakers.json").read_text()) if (d / "speakers.json").exists() else {}
last = {}
for raw in (d / "lines.jsonl").read_text().splitlines():
    try: l = json.loads(raw)
    except ValueError: continue
    last[l["id"]] = l
for l in sorted(last.values(), key=lambda l: l["t0"]):
    s = l["speaker"]
    if s["source"] != "system": continue
    dur = (l["t1"] - l["t0"] - 600) / 1000  # sem o silêncio de fechamento
    label = sp.get(s.get("voice", ""), {}).get("label", s["label"])
    print(f'{l["id"]:>5} {dur:5.1f}s {s.get("voice","-"):>4} {label:<20} {l["text"][:70]}')
PY
```
Marque, para cada fala de 1 s ou mais, se o rótulo está certo. Conte também: quantas vezes uma pessoa virou dois grupos e quantas vezes duas pessoas ficaram num grupo só.

- [ ] **Step 3: Reunião 2 — reconhecimento**

Numa segunda reunião com as mesmas pessoas (outro dia, se possível): conte quantas vozes nomeadas na reunião 1 apareceram já com o nome, e quantas vezes um nome apareceu na pessoa errada. Meça CPU (Monitor de Atividade, processo gow-meet, média por ~5 min de fala) e o atraso percebido, como na etapa 1.

- [ ] **Step 4: Checagens de privacidade**

Run:
```bash
A=~/Library/Application\ Support/gow-meet
ls "$A"/voices.json*                                    # só voices.json (e cópias .corrompido-*, se houve)
grep -l embedding "$A"/sessions/*/* 2>/dev/null | wc -l  # 0
NOME="o primeiro nome que você deu na reunião 1"         # só no terminal, não vai para nenhum arquivo
grep -ci "$NOME" "$A"/logs/*.log                         # 0 em todos
```
Expected: `voices.json` só na pasta local; `0` arquivos de sessão com impressão; `0` ocorrências de nomes nos logs.

- [ ] **Step 5: Registrar (só números)**

`docs/validacao-falantes.md` — nunca nomes, falas ou trechos da reunião:
```markdown
# Validação manual — quem falou (etapa 2)

Máquina: Mac M2 8 GB · macOS: <versão> · Build: <commit> · Data: <data>
Modelo: wespeaker-ecapa512-lm · Limites: biblioteca 0,70 / grupo 0,65 / mínimo 1000 ms

## Reunião 1 (vozes novas)

| Métrica | Valor |
|---|---|
| Participantes remotos | |
| Falas do sistema com 1 s ou mais | |
| … com o rótulo certo | n (%) — meta ≥ 85% |
| Uma pessoa virou dois grupos (vezes) | |
| Duas pessoas num grupo só (vezes) | |
| Falas curtas (< 1 s) com rótulo errado | |

## Reunião 2 (reconhecimento)

| Métrica | Valor |
|---|---|
| Vozes nomeadas na reunião 1 que participaram | |
| Reconhecidas pelo nome logo na primeira fala longa | n de m |
| Nome na pessoa errada (vezes) | |

## Desempenho (comparar com docs/validacao-mac.md)

| Métrica | Etapa 1 | Etapa 2 |
|---|---|---|
| CPU média durante a fala (%) | | |
| Atraso percebido da tradução | | |
| Avisos "Atraso de N s" na reunião | | |

## Privacidade

| Checagem | Resultado |
|---|---|
| voices.json só na pasta local | |
| Arquivos da sessão com impressão de voz | 0 |
| Nomes nos logs | 0 |
| "Apagar todas" remove voices.json e cópias | |

## Windows (notebook do time)

| Checagem | Resultado |
|---|---|
| cargo test -p gow-meet-core | |
| speaker_say (3 testes ignorados, fixtures copiadas do Mac) | |
| voices.json em %LOCALAPPDATA%\gow-meet | |
| CI: tract/rustfft no MSVC | |

Calibração: <limites testados e efeito, ou "padrões mantidos">
Pendências encontradas: <lista ou "nenhuma">
```
Se a meta de 85% não for atingida, olhe os números antes de mexer: "uma pessoa virou dois grupos" alto → baixe `speaker_group_threshold` (ex.: 0,60) no `config.json` e repita numa próxima reunião; "duas pessoas num grupo" alto → suba (ex.: 0,70). Se um novo padrão for adotado, mude `TrackerConfig::default()` (`crates/core/src/tracker.rs`), o padrão em `src-tauri/src/config.rs`, a tabela do `README.md` e rode `cargo test --workspace` (o teste `speaker_defaults_and_override` compara os dois). Cada problema encontrado vira um teste que o reproduz (sem hardware, com impressões falsas, quando possível) e uma correção em commit próprio.

- [ ] **Step 6: Verificação no Windows (junto com as pendências da etapa 1)**

No notebook Windows do time, quando a Task 14 da etapa 1 (captura WASAPI e build) for retomada: copie `target/voice-fixtures/` e `target/voice-models/voxceleb_ECAPA512_LM.onnx` do Mac (o `say` não existe no Windows) e rode em PowerShell:
```powershell
$env:CMAKE_POLICY_VERSION_MINIMUM = "3.5"
cargo test -p gow-meet-core
$env:GOW_MEET_SPEAKER_MODEL = "$PWD\target\voice-models\voxceleb_ECAPA512_LM.onnx"
$env:GOW_MEET_VOICES = "$PWD\target\voice-fixtures"
cargo test -p gow-meet-core --release --test speaker_say -- --ignored --nocapture
```
Expected: os mesmos resultados do Mac (0 erradas). Com o app rodando, dê nome a uma voz e confira que `voices.json` foi criado em `%LOCALAPPDATA%\gow-meet` e não em `%APPDATA%`. Preencha a seção "Windows" do documento.

- [ ] **Step 7: Commit**

```bash
git add docs/validacao-falantes.md
git commit -m "Registra a validação manual da identificação de falantes" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01SG9JDcmH4p1AX9zNbctMF2"
```

---

## Marcos visíveis

1. Task 2: `cargo test --test speaker_say -- --ignored` separa as vozes do `say` com o tract (primeiro número real de tempo e qualidade).
2. Task 5: o CLI e o pipeline já gravam `speaker.voice` e `speakers.json`; o teste pelo pipeline reconhece as vozes numa segunda reunião.
3. Task 6: no app, as falas dos participantes aparecem como "PARTICIPANTE 1", "PARTICIPANTE 2"…
4. Task 7: dar nome na flutuante e gerenciar as vozes na janela principal.
5. Task 8: números de uma reunião real.

## Autorrevisão

- **Cobertura da spec:** §1 critério de sucesso → Task 8; restrições → Global Constraints. §2 decisões → Tasks 3 (memória, renomear não altera a voz reconhecida), 5 (só sistema), 6 (pasta local), 7 (nomear durante a reunião, tela de exclusão). §3 componentes → Tasks 2 (`SpeakerEmbedder`), 3 (`VoiceLibrary`, `SpeakerTracker`), 5 (integração na thread de transcrição, fala final já com rótulo, microfone de fora, fala curta herda), 6 (`config.json`); modelo → Task 1. §4 fluxo → Tasks 5 e 6. §5 armazenamento → Tasks 3 (voices.json atômico, corrompido preservado, impressão só lá) e 4 (`voice`, `speakers.json`, Markdown, sessões antigas). Logs → Task 3/5 (só ids e contagens) e teste `voice_privacy` (Task 5). §6 interface → Task 7; contrato → Tasks 5 e 6. §7 falhas: modelo ausente/falha → `speaker_setup` + `SPEAKER_UNAVAILABLE` (Task 6); erro na impressão → `note_embed_failure` + herança (Tasks 3, 5); `voices.json` corrompido → Task 3 + `VOICES_CORRUPT` (Task 6); falha ao gravar → `Store::set_speakers`/`save_library` com aviso e nova tentativa (Task 5); renomear sem sessão/voz inexistente → erros de `rename_voice` (Tasks 5, 6). §8 testes → Tasks 2–7 (unitários), `speaker_say` (integração manual), CI (Task 1, Step 11), validação (Task 8). §9 ordem → Tasks 1–8. §10 riscos → limites configuráveis (Task 6), CI Windows (Task 1), LGPD (README, Task 6).
- **Placeholders:** os únicos campos em branco são os dos documentos de resultados (`docs/modelo-de-voz.md`, `docs/validacao-falantes.md`), preenchidos com medições.
- **Consistência de nomes:** `SpeakerEmbedder`/`TractEmbedder` (Task 2) → Tasks 3, 5, 6; `VoiceLibrary`, `VoiceSummary`, `VOICES_FILE`, `SpeakerTracker`, `TrackerConfig`, `Assignment`, `SpeakerEntry`, `Speakers` (Task 3) → Tasks 4, 5, 6; `Speaker::tracked`, `read_speakers`, `set_speakers` (Task 4) → Task 5; `SpeakerId`, `start_with_speakers`, `rename_voice`, `PipelineEvent::Speaker`, `UiEvent::Speaker` (Task 5) → Tasks 6, 7; comandos e `speaker {voice,label}` (Task 6) → Task 7; id `wespeaker-ecapa512-lm` (Task 1) → Tasks 2, 6, 8.
- **Verificação prévia:** o código Rust das Tasks 2–6 foi compilado e testado numa cópia do repositório antes de escrever o plano (98 testes da `gow-meet-core`, `voice_privacy`, os 3 testes `speaker_say` com o ECAPA, `cargo check` do app); o front-end da Task 7 passou em `tsc -b`, ESLint, Vitest (20 testes) e `vite build`.
