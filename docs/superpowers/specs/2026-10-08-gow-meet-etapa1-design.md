# gow-meet — Etapa 1: tradução simultânea e transcrição local

Data: 2026-10-08 · Nome provisório: **gow-meet** · Status: aguardando revisão

## 1. Contexto e objetivo

Ferramenta **interna da gow** (não comercializada) para auxiliar reuniões com
clientes externos. Visão final do produto, em três etapas independentes:

| Etapa | Entrega |
|---|---|
| **1. Tradução ao vivo** (este documento) | Captura áudio, transcreve, traduz numa janela flutuante e salva a transcrição localmente |
| 2. Quem falou | Identificar o participante de cada frase (diarização e/ou nomes do app de reunião) |
| 3. Notas | Gerar documentos a partir da transcrição e de uma instrução posterior, no estilo do `/meeting-report` |

Este documento cobre **somente a etapa 1**. As etapas 2 e 3 terão spec e plano próprios.

### Critério de sucesso da etapa 1

Um usuário acompanha uma reunião real em inglês (app instalado do Teams ou
qualquer outro) pela janela flutuante, com tradução para português, e ao
final encontra a transcrição completa salva — no Mac e num notebook Windows
do time.

### Restrições

- Funciona com **qualquer app de reunião**, incluindo o Teams instalado: captura
  o áudio do sistema, não depende de navegador.
- **Processamento 100% local** nesta etapa: o áudio do cliente não sai da máquina.
  A arquitetura permite provedores em nuvem depois, mediante decisão da gow.
- **O áudio não é guardado.** Só texto (original + tradução).
- Modelos devem ter licença que permita uso em empresa (ex.: Whisper MIT,
  Opus-MT CC-BY 4.0). NLLB está **excluído** (não comercial).
- Hardware do time: maioria **Windows com RTX 2050 (4 GB VRAM)**. Desenvolvimento
  inicial num **Mac M2 com 8 GB**.

### Fora do escopo da etapa 1

Nomes reais ou separação de vozes; notas com IA; tradução pt→en; Linux;
atualização automática; assinatura/notarização para distribuição; uso por clientes.

## 2. Decisões tomadas

| Tema | Decisão |
|---|---|
| Plataforma | **Mac primeiro**, Windows em seguida. Linux só se houver demanda |
| Tecnologia | **Tauri v2**: núcleo em Rust, interface em TypeScript (React + Vite) |
| Transcrição | whisper.cpp (via `whisper-rs`), GPU por Metal (Mac) e Vulkan ou CUDA (Windows, definido no spike) |
| Tradução | Opus-MT en→pt (`Helsinki-NLP/opus-mt-tc-big-en-pt`) convertido para CTranslate2, via `ct2rs` |
| Detecção de voz | VAD Silero integrado ao whisper.cpp (evita dependência de ONNX Runtime) |
| Idiomas | en→pt. A direção é configuração, não está fixa no código; pt→en futuro |
| Janela flutuante | Tradução em destaque, original menor embaixo; parcial em cinza |
| Quem falou | Microfone = "Você"; áudio do sistema = "Participante" |
| Áudio | Não é guardado |
| Modelos | Baixados no primeiro uso do Hugging Face, versão fixa e hash verificado; URL base configurável |
| Repositório | Novo repositório **gow-meet no GitHub**, CI com GitHub Actions |
| Licença | Definida pela gow. Nenhum código deste repositório GPL é reaproveitado; só ideias e formato de dados |

## 3. Arquitetura

```text
 Alto-falante (tap do sistema) ─┐
 Microfone ─────────────────────┤
                                ▼
                       [audio] captura, 16 kHz mono, um fluxo por fonte
                                ▼
                       [segmenter] detecta voz e corta frases
                                ▼
                       [asr] whisper.cpp na GPU → texto em inglês
                                ▼
                       [translate] Opus-MT en→pt → texto em português
                                ▼
                       [session] identifica as falas e grava em disco
                                ▼  eventos do Tauri
             ┌──────────────────┴──────────────────┐
    [overlay] janela flutuante             [main] janela principal
```

Cada módulo Rust expõe uma interface (trait) e pode ser testado isoladamente.

| Módulo | Responsabilidade | Implementações |
|---|---|---|
| `audio` | Captura o áudio do sistema e o microfone em fluxos separados, reamostrados para 16 kHz mono; detecta troca de dispositivo e reconecta | Mac: Core Audio process taps (macOS 14.2+) + microfone via `cpal`. Windows: WASAPI loopback + microfone via `cpal` |
| `segmenter` | Detecta fala; emite janela parcial a cada ~1 s; fecha a frase no silêncio | Único |
| `asr` | Transcreve áudio → texto + tempos | whisper.cpp; modelo escolhido no spike (candidatos: `small.en`, `large-v3-turbo` quantizado) |
| `translate` | Texto → tradução; recebe par de idiomas | Opus-MT/CTranslate2. Trait permite provedor em nuvem no futuro |
| `session` | Atribui `id`, tempos e `speaker`; grava `lines.jsonl`, `meta.json` e gera `transcript.md`; recupera sessões interrompidas | Único |
| `models` | Download, verificação de hash e cache dos modelos | Único |
| `ui` | Janela flutuante (sempre no topo, translúcida, arrastável; no Mac via plugin NSPanel para ficar sobre apps em tela cheia) e janela principal (iniciar/parar, configurações, lista de reuniões, lembrete de consentimento) | TypeScript/React |

## 4. Fluxo de dados

1. Enquanto a pessoa fala, a parcial é retranscrita a cada ~1 s com o **mesmo `id`**
   e aparece em cinza na janela flutuante.
2. No silêncio, a fala é **fechada**: retranscrita com o áudio completo e enviada
   para tradução.
3. A tradução chega depois e **atualiza a fala pelo `id`**, nunca duplica.
4. Traduções de parciais são limitadas a no máximo uma a cada ~1,5 s.
5. Os dois fluxos (microfone e sistema) passam pelo pipeline de forma independente;
   o `speaker.source` indica a origem.

## 5. Armazenamento

Uma pasta por reunião:

- Mac: `~/Library/Application Support/gow-meet/sessions/<id>/`
- Windows: `%APPDATA%\gow-meet\sessions\<id>\`

`<id>` tem o formato `2026-10-08_143210` (data e hora local de início).

| Arquivo | Conteúdo |
|---|---|
| `meta.json` | `v`, `id`, `title` (editável), `started`, `ended` (nulo enquanto ativa), `source_lang`, `target_lang`, modelos e versões, versão do app |
| `lines.jsonl` | Somente acréscimo. Um registro por fala fechada e um por atualização de tradução; o último registro de cada `id` prevalece |
| `transcript.md` | Versão legível, gerada ao encerrar; sempre recriável a partir de `lines.jsonl` |

Registro de exemplo (dados fictícios):

```json
{"id":"f42","t0":183200,"t1":187900,"speaker":{"source":"system","label":"Participante"},
 "text":"Let's review the timeline.","translation":"Vamos revisar o cronograma."}
```

- `t0`/`t1`: milissegundos desde o início da reunião.
- `speaker` é objeto para a etapa 2 acrescentar identificação sem mudar o formato.
- `translation` pode ser `null` (pendente) ou ausente com `translation_error: true`.
- `meta.json` e `transcript.md` são escritos de forma atômica (arquivo temporário +
  renomeação). `lines.jsonl` é acrescentado com flush por registro.
- As etapas 2 e 3 leem esta mesma pasta.

## 6. Tratamento de falhas

| Situação | Comportamento |
|---|---|
| Permissão negada (áudio do sistema ou microfone) | Explicação na janela principal com botão para os Ajustes do Sistema; funciona com só uma fonte |
| Troca de dispositivo de áudio | Reconexão automática; overlay mostra "reconectando áudio…" |
| Transcrição mais lenta que a fala | Descarta parciais primeiro, nunca falas fechadas; aviso com o atraso acima de ~5 s |
| Falha de tradução numa fala | Mostra o original, marca "sem tradução", segue para a próxima |
| GPU indisponível | Usa o processador e avisa que ficará mais lento |
| Modelos ausentes | Download no primeiro uso com progresso e verificação de hash; depois funciona offline |
| App fecha no meio da reunião | Na próxima abertura, sessões sem `ended` são encerradas e o `transcript.md` é gerado |
| Erro de escrita em disco | Aviso visível; falas mantidas em memória e nova tentativa |

**Logs** vão para arquivo próprio com rotação e **nunca contêm texto das reuniões**
(só eventos, tempos e erros).

**Consentimento:** ao iniciar, a janela principal mostra um lembrete curto para
avisar os participantes de que a reunião será transcrita. Não bloqueia o uso.
A regra de uso (LGPD, contratos de clientes) é política da gow, fora do software.

## 7. Testes e validação

### Automáticos (sem dados reais)

- **Áudio sintético** gerado com `say` (macOS) a partir de frases fictícias com texto
  conhecido; mede WER para detectar regressão.
- **Tradução:** conjunto fixo de frases fictícias; verifica saída em português e
  ausência de falas sem resposta.
- **Sessão:** queda simulada no meio do `lines.jsonl` e recuperação; tradução que
  chega depois atualiza pelo `id` sem duplicar; geração do Markdown.
- **Segmentação e fila:** falas fecham no silêncio; sob atraso, só parciais são descartadas.
- **CI (GitHub Actions):** compila e roda os testes em macOS e Windows a cada alteração.
  Os runners não têm GPU: CI cobre a lógica e o caminho por processador; desempenho
  em GPU é medido no spike e na validação manual.

### Spike de viabilidade no Windows (descartável, no início)

Num notebook do time com RTX 2050, com 10 minutos de áudio sintético a partir de
arquivo (sem captura real):

| Métrica | Meta |
|---|---|
| Atraso de fala fechada (fim da fala → tradução na tela) | ≤ 3 s no p90 |
| Atraso de parciais | ≤ 1,5 s |
| Memória de vídeo | ≤ 3 GB |
| Uso de processador | ≤ 30% |

Também decide entre Vulkan e CUDA e escolhe o modelo Whisper. Se as metas não forem
atingidas, a escolha de modelo é revista antes da captura no Windows.

### Validação manual

No Mac, numa reunião real pelo app do Teams: tradução acompanha a conversa; troca de
fone não quebra a captura; transcrição salva confere; uso de CPU em repouso é baixo.
Depois, o mesmo num notebook Windows do time. O relatório registra o que foi
efetivamente testado; testes automáticos não bastam para declarar compatibilidade.

## 8. Ordem de entrega

1. Repositório, esqueleto Tauri e CI macOS + Windows.
2. Spike de viabilidade no Windows (pode correr em paralelo ao item 3).
3. Pipeline com áudio de arquivo: `segmenter` → `asr` → `translate` → `session`, com testes.
4. Download de modelos.
5. Captura no Mac (tap do sistema + microfone) e permissões.
6. Janela flutuante e janela principal.
7. Validação manual no Mac.
8. Captura no Windows (WASAPI) e validação num notebook do time.

## 9. Riscos

| Risco | Mitigação |
|---|---|
| RTX 2050 em notebook com limite de energia não atinge o tempo real | Spike logo no início; modelos menores/quantizados |
| 8 GB de memória no Mac de desenvolvimento, com Teams aberto | Modelo Whisper menor; medir uso de memória |
| Core Audio taps exigem API recente e permissões que mudam entre versões do macOS | Implementação isolada no módulo `audio`; checagem de permissões na interface |
| Permissões do macOS esquecidas a cada build sem assinatura estável | Assinar builds de desenvolvimento com certificado fixo |
| Qualidade de tradução local inferior à nuvem | Trait de tradução permite provedor em nuvem futuro, mediante decisão da gow |
| Rede de cliente bloqueia Hugging Face | URL base dos modelos configurável |
