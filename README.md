# GOW Meet

Legendas ao vivo e traduzidas para reuniões, rodando inteiramente no computador. Escolha o
idioma falado antes de começar — **inglês** (tradução para o português) ou **português**
(tradução para o inglês) — e troque no meio da reunião se a conversa mudar de idioma.

O GOW Meet captura o áudio do sistema (o que os participantes falam) e o microfone (o que
você fala), transcreve com o Whisper e traduz com o Opus-MT. As falas aparecem numa janela
flutuante, separadas por voz (**Você**, **Participante 1**, **Participante 2**…); clique no
rótulo para dar nome a uma voz, e o app a reconhece nas próximas reuniões, neste computador.
Ao encerrar, a transcrição fica salva em Markdown.

**O áudio nunca é gravado em disco.** Só o texto (original e tradução) é salvo, e os logs
não contêm o conteúdo das falas.

## Download

**[Baixar a versão mais recente](https://github.com/washjunio/gow-meet/releases/latest)**

| Sistema | Arquivo |
|---|---|
| macOS 13 ou mais novo (Apple Silicon) | `GOW Meet_<versão>_aarch64.dmg` |
| Windows 10/11 (64 bits) | `GOW Meet_<versão>_x64-setup.exe` |

O app não tem assinatura da Apple nem da Microsoft, então o sistema avisa na primeira
abertura:

- **macOS:** abra **Ajustes do Sistema → Privacidade e Segurança** e clique em **Abrir
  Mesmo Assim**. A cada versão nova, o macOS pede de novo as permissões de microfone e de
  gravação de tela.
- **Windows:** clique em **Mais informações → Executar assim mesmo**.

## Como usar

O **[tutorial](docs/tutorial.md)** mostra, com imagens, como instalar, fazer a primeira
reunião, dar nome a quem fala, trocar de idioma, abrir a transcrição e gerenciar as vozes
guardadas.

## Publicar uma versão

Atualize `version` em `src-tauri/tauri.conf.json` e envie a tag correspondente
(`git tag v0.1.1 && git push origin v0.1.1`). O workflow `release` gera o `.dmg` do macOS
e o instalador do Windows e cria a Release com os dois. Se a tag não bater com a versão,
ele para com erro.

Para desenvolver, veja as seções abaixo. Aplicativo desktop em Tauri 2 + React.

## Pré-requisitos

- Rust (stable, 1.90 ou mais novo) — <https://rustup.rs>
- CMake (para compilar o whisper.cpp e o CTranslate2). Com CMake 4.x, exporte antes de
  compilar: `export CMAKE_POLICY_VERSION_MINIMUM=3.5`
- Node.js 20
- macOS: Xcode Command Line Tools (`xcode-select --install`)
- Windows: Visual Studio Build Tools com "Desenvolvimento para desktop com C++"

## Como rodar

```sh
npm install
export CMAKE_POLICY_VERSION_MINIMUM=3.5
npx tauri dev
```

Na primeira vez que você clicar em **Iniciar**, os modelos (~700 MB) são baixados e
verificados (SHA-256); depois disso o app funciona offline.

Testes: `cargo test --workspace` e `npm test`.

## CLI

O `gow-meet-cli` roda o mesmo pipeline no terminal, útil para medir latência e qualidade:

```sh
# sobre um arquivo WAV, em ritmo real
cargo run --release -p gow-meet-cli -- entrada.wav \
  --whisper <pasta-de-modelos>/whisper-small-en/ggml-small.en-q5_1.bin \
  --opus <pasta-de-modelos>/opus-mt-en-pt \
  --prefix '>>pob<<'

# reunião em português (tradução para o inglês, sem prefixo)
cargo run --release -p gow-meet-cli -- entrada.wav --lang pt \
  --whisper <pasta-de-modelos>/whisper-small/ggml-small-q5_1.bin \
  --opus <pasta-de-modelos>/opus-mt-pt-en

# ao vivo: áudio do sistema + microfone (Ctrl+C encerra)
cargo run --release -p gow-meet-cli -- --live \
  --whisper <...>.bin --opus <pasta-opus> --prefix '>>pob<<'
```

Outras opções: `--cpu` (sem GPU), `--fast` (sem esperar o tempo real), `--repeat N`,
`--out <pasta>` (padrão `gow-meet-cli-out`). Veja `--help`.

## Permissões do macOS

Em **Ajustes do Sistema → Privacidade e Segurança**, habilite o GOW Meet (ou o Terminal,
ao rodar pelo `tauri dev`/CLI) em:

- **Microfone** — para transcrever a sua voz;
- **Gravação de Tela e Áudio do Sistema** — para capturar o áudio dos participantes (só o
  áudio é usado; nenhuma imagem da tela é guardada).

Se faltar alguma permissão, a janela principal mostra um aviso com o botão que abre o
painel certo. Depois de habilitar, inicie a reunião de novo.

## Use fones de ouvido

No alto-falante, a fala do participante também entra pelo microfone e aparece duplicada
como "Você". Nesta etapa não há cancelamento de eco: use fones de ouvido.

## Onde ficam os dados

`<dados>` é `~/Library/Application Support/gow-meet` no macOS e
`%APPDATA%\gow-meet` (Roaming) no Windows. `<locais>` é a mesma pasta no macOS e
`%LOCALAPPDATA%\gow-meet` no Windows, para que os modelos e logs não entrem no perfil móvel.

| O quê | Onde |
| --- | --- |
| Reuniões (`meta.json`, `lines.jsonl`, `transcript.md`) | `<dados>/sessions/<id>/` |
| Nomes das vozes de cada reunião (`speakers.json`) | `<dados>/sessions/<id>/` |
| Configuração | `<dados>/config.json` |
| Vozes guardadas (`voices.json`) | `<locais>/voices.json` |
| Modelos | `<locais>/models/<id-do-modelo>/` |
| Logs (rotação diária, 7 arquivos) | `<locais>/logs/` |

Uma reunião interrompida (queda do app) é fechada automaticamente na próxima abertura.

### config.json

Opcional; chaves ausentes usam o padrão. O idioma falado é escolhido na janela principal
antes de iniciar e pode ser trocado durante a reunião (janela principal ou flutuante).

| Chave | Padrão | Descrição |
| --- | --- | --- |
| `whisper_model` | `"whisper-small-en"` | Modelo de transcrição para reuniões em inglês (`whisper-small-en` ou `whisper-turbo`) |
| `opus_model` | `"opus-mt-en-pt"` | Modelo de tradução inglês → português |
| `opus_prefix` | `">>pob<<"` | Prefixo de idioma de destino do Opus-MT en→pt (português do Brasil) |
| `whisper_model_pt` | `"whisper-small"` | Modelo de transcrição para reuniões em português |
| `opus_model_pt` | `"opus-mt-pt-en"` | Modelo de tradução português → inglês |
| `opus_prefix_pt` | nenhum | Prefixo de idioma de destino do Opus-MT pt→en |
| `models_base_url` | `"https://huggingface.co"` | De onde baixar os modelos (ex.: um espelho interno) |
| `speaker_model` | `"wespeaker-ecapa512-lm"` | Modelo de impressão de voz (quem falou) |
| `speaker_library_threshold` | `0.70` | Semelhança mínima para reconhecer uma voz guardada |
| `speaker_group_threshold` | `0.65` | Semelhança mínima para juntar a fala a uma voz da reunião |
| `speaker_min_ms` | `1000` | Falas mais curtas herdam o último participante que falou |

A variável de ambiente `GOW_MEET_MODELS_URL` tem precedência sobre `models_base_url`. O
download usa os certificados e o proxy configurados no sistema.

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

## Modelos e licenças

| Modelo | Uso | Licença |
| --- | --- | --- |
| whisper.cpp `small.en` / `small` (multilíngue) / `large-v3-turbo` | transcrição | MIT |
| Opus-MT en→pt (convertido para CTranslate2) | tradução | CC BY 4.0 |
| Opus-MT pt→en (`Helsinki-NLP/opus-mt-itc-en`, convertido para CTranslate2) | tradução | Apache-2.0 |
| WeSpeaker ECAPA-TDNN512-LM (`Wespeaker/wespeaker-ecapa-tdnn512-LM`, treinado no VoxCeleb2) | impressão de voz (quem falou) | CC BY 4.0 — © projeto WeSpeaker (wenet-e2e) |
