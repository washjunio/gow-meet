# gow-meet

Legendas ao vivo para reuniões em inglês, traduzidas para o português (pt-BR), rodando
inteiramente no computador. O gow-meet captura o áudio do sistema (o que os participantes
falam) e o microfone (o que você fala), transcreve o inglês com o Whisper e traduz com o
Opus-MT. As falas aparecem numa janela flutuante, identificadas como **Você** (microfone) e
**Participante** (áudio do sistema), e a transcrição fica salva em Markdown ao encerrar.

Aplicativo desktop em Tauri 2 + React. Etapa 1: macOS primeiro; Windows em seguida.

**O áudio nunca é gravado em disco.** Só o texto (original e tradução) é salvo, e os logs
não contêm o conteúdo das falas.

## Pré-requisitos

- Rust (stable, 1.90 ou mais novo) — <https://rustup.rs>
- CMake (para compilar o whisper.cpp e o CTranslate2). Com CMake 4.x, exporte antes de
  compilar: `export CMAKE_POLICY_VERSION_MINIMUM=3.5`
- Node.js 20
- macOS: Xcode Command Line Tools (`xcode-select --install`)

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

# ao vivo: áudio do sistema + microfone (Ctrl+C encerra)
cargo run --release -p gow-meet-cli -- --live \
  --whisper <...>.bin --opus <pasta-opus> --prefix '>>pob<<'
```

Outras opções: `--cpu` (sem GPU), `--fast` (sem esperar o tempo real), `--repeat N`,
`--out <pasta>` (padrão `gow-meet-cli-out`). Veja `--help`.

## Permissões do macOS

Em **Ajustes do Sistema → Privacidade e Segurança**, habilite o gow-meet (ou o Terminal,
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
| Configuração | `<dados>/config.json` |
| Modelos | `<locais>/models/<id-do-modelo>/` |
| Logs (rotação diária, 7 arquivos) | `<locais>/logs/` |

Uma reunião interrompida (queda do app) é fechada automaticamente na próxima abertura.

### config.json

Opcional; chaves ausentes usam o padrão.

| Chave | Padrão | Descrição |
| --- | --- | --- |
| `source_lang` | `"en"` | Idioma falado na reunião |
| `target_lang` | `"pt"` | Idioma da tradução |
| `whisper_model` | `"whisper-small-en"` | Modelo de transcrição (`whisper-small-en` ou `whisper-turbo`) |
| `opus_model` | `"opus-mt-en-pt"` | Modelo de tradução |
| `opus_prefix` | `">>pob<<"` | Prefixo de idioma de destino do Opus-MT (português do Brasil) |
| `models_base_url` | `"https://huggingface.co"` | De onde baixar os modelos (ex.: um espelho interno) |

A variável de ambiente `GOW_MEET_MODELS_URL` tem precedência sobre `models_base_url`. O
download usa os certificados e o proxy configurados no sistema.

## Modelos e licenças

| Modelo | Uso | Licença |
| --- | --- | --- |
| whisper.cpp `small.en` / `large-v3-turbo` | transcrição | MIT |
| Opus-MT en→pt (convertido para CTranslate2) | tradução | CC BY 4.0 |
| WeSpeaker ECAPA-TDNN512-LM (`Wespeaker/wespeaker-ecapa-tdnn512-LM`, treinado no VoxCeleb2) | impressão de voz (quem falou) | CC BY 4.0 — © projeto WeSpeaker (wenet-e2e) |
