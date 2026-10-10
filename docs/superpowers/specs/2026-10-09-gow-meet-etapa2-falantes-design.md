# gow-meet — Etapa 2: quem falou

Data: 2026-10-09 · Status: aguardando revisão · Base: etapa 1 (branch `etapa1`)

## 1. Contexto e objetivo

A etapa 1 separa as falas só pela origem do áudio: microfone → "Você",
áudio do sistema → "Participante". Numa reunião com vários participantes
remotos, todos aparecem como "Participante", na legenda e no `transcript.md`.

A etapa 2 identifica **qual participante remoto** disse cada fala, permite
**dar nome** a cada voz durante a reunião e **lembra das vozes nomeadas** nas
próximas reuniões, no mesmo computador.

### Critério de sucesso

Numa reunião real com pelo menos 3 participantes remotos:

- ≥ 85% das falas de 1 s ou mais aparecem com o rótulo certo (o mesmo
  participante sempre com o mesmo rótulo, participantes diferentes com
  rótulos diferentes);
- depois de nomeada, a voz de uma pessoa é reconhecida pelo nome numa segunda
  reunião;
- CPU e atraso da tradução sem piora perceptível em relação à etapa 1.

### Restrições (herdadas da etapa 1)

- Processamento 100% local. Nada de áudio ou impressão de voz sai da máquina.
- O áudio nunca é gravado em disco.
- Logs nunca contêm texto de reunião nem nomes de pessoas.
- Modelos com licença que permita uso em empresa.
- Mac primeiro, Windows em seguida.

### Fora do escopo da etapa 2

- Corrigir fala a fala ("esta foi o Bruno"), juntar ou separar grupos.
- Dar nomes depois da reunião (só durante).
- Identificar vozes no microfone: o microfone continua sempre "Você",
  inclusive pessoas na mesma sala.
- Separar falantes dentro de uma mesma fala (troca de voz sem pausa) e falas
  sobrepostas.
- Compartilhar vozes entre computadores do time.
- Nomes vindos do app de reunião (Teams etc.).
- Direção de tradução pt→en (pergunta em aberto, tratada à parte).

## 2. Decisões tomadas

| Tema | Decisão |
|---|---|
| Abordagem | Impressão de voz por fala fechada + agrupamento em tempo real (abordagem A). Diarização completa (pyannote) e o marcador de turno do Whisper foram descartados |
| Onde aplica | Só ao áudio do sistema |
| Correção | Só dar nome às vozes |
| Quando nomear | Durante a reunião, clicando no rótulo na janela flutuante |
| Memória | O app lembra das vozes nomeadas entre reuniões |
| Consentimento | Responsabilidade da gow, por processos próprios, fora do software. O app guarda toda voz que recebe nome. A regra de uso (LGPD: impressão de voz é dado biométrico, dado pessoal sensível) é política da gow |
| Armazenamento das vozes | Só no computador de cada pessoa, na pasta local do app (não no perfil que sincroniza) |
| Exclusão | Tela para ver, renomear e apagar vozes guardadas, para atender pedidos de exclusão |

## 3. Arquitetura

Componentes novos em `gow-meet-core`, cada um com uma responsabilidade:

- **`SpeakerEmbedder`** (trait): recebe o áudio de uma fala (16 kHz mono) e
  devolve a impressão de voz (vetor de `f32`, normalizado). Implementação real
  com um modelo ONNX pequeno e local; implementação falsa nos testes.
- **`VoiceLibrary`**: vozes guardadas (`id`, `name`, `embedding`, `created`,
  `updated`, `count`). Carrega e grava `voices.json` de forma atômica.
- **`SpeakerTracker`**: decide o rótulo de cada fala do sistema.
  1. compara a impressão com a biblioteca; se a semelhança (cosseno) passar
     do limite, usa o nome da voz guardada;
  2. senão, compara com os grupos da reunião atual; se passar do limite, usa
     o rótulo do grupo e atualiza o centroide do grupo;
  3. senão, cria um grupo novo "Participante N".

Integração no pipeline (`pipeline.rs`): na thread de transcrição, logo depois
do Whisper, para falas finais do sistema. A fala final já sai com o rótulo
certo; não há troca visível de "Participante" para "Ana". O microfone pula
essa etapa.

Falas curtas: abaixo da duração mínima (padrão ~1 s), a fala herda o rótulo
do último falante do sistema na reunião, sem criar grupo nem atualizar
centroide.

Ao dar nome a um grupo durante a reunião:

- o grupo ganha o nome; a impressão do grupo é gravada na biblioteca
  (criando a voz ou somando à voz de mesmo nome já existente);
- se o grupo tinha sido reconhecido como outra voz guardada (ex.: "Ana") e o
  usuário escreve outro nome ("Bruno"), a voz "Ana" guardada **não muda**; o
  grupo passa a apontar para "Bruno";
- as falas anteriores do grupo passam a mostrar o nome na tela e no arquivo.

Ao fim da reunião, as vozes reconhecidas ou nomeadas têm a impressão da
biblioteca atualizada por média ponderada (`count`).

Configuração (`config.json`, com padrões): limite de semelhança para
biblioteca, limite para grupos da reunião, duração mínima, id do modelo de
voz. Os padrões são calibrados na validação.

Modelo de voz: escolhido na primeira tarefa do plano entre 2 ou 3 candidatos,
por licença (uso em empresa), tamanho, tempo por fala no M2 e qualidade no
teste com vozes do `say`. Baixado do Hugging Face e conferido por hash, como
os demais (manifesto com revisão fixa). Roda com ONNX Runtime.

## 4. Fluxo de dados

```text
áudio do sistema → segmentador → fala final → Whisper → texto
                                            ↘ SpeakerEmbedder → SpeakerTracker → rótulo + voice
                                    → Final(Line com speaker.voice) → tradução → sessão / UI
UI: clique no rótulo → rename_voice{voice,name} → tracker + speakers.json + voices.json
                                                → evento speaker{voice,label} → UI atualiza falas
```

## 5. Armazenamento

### Sessão (`<data_dir>/gow-meet/sessions/<id>/`)

- `lines.jsonl`: `speaker` ganha o campo opcional `voice` (id do grupo na
  reunião: `"v1"`, `"v2"`…). Microfone sem `voice`, rótulo "Você". Arquivo
  continua só acrescentando.
- `speakers.json` (novo, escrita atômica): `{ "v1": {"label": "Ana",
  "voice_id": "<id da biblioteca>"}, "v2": {"label": "Participante 2"} }`.
  Renomear atualiza só este arquivo.
- `transcript.md`: gerado usando os rótulos de `speakers.json` (inclusive
  para falas anteriores à renomeação), no fim da reunião e na recuperação.
- Sessões antigas, sem `voice` nem `speakers.json`, continuam funcionando
  como na etapa 1.

### Biblioteca (`voices.json`)

- Na pasta **local** do app (Mac: Application Support; Windows:
  `%LOCALAPPDATA%`), como os modelos, não no perfil que sincroniza.
- Escrita atômica. Arquivo corrompido: aviso sem conteúdo no log, biblioteca
  vazia, arquivo original preservado com sufixo para não ser sobrescrito.
- Impressões de voz existem **só** neste arquivo e na memória durante a
  reunião. Nunca nos arquivos da sessão, nos logs ou na UI.
- Apagar uma voz remove a entrada. Transcrições antigas mantêm o nome como
  texto.

### Logs

Só contagens e identificadores (ex.: "voz reconhecida v1 → <id>"). Nunca
nomes.

## 6. Interface

### Janela flutuante

- Rótulo das falas do sistema clicável (ícone de lápis discreto ao passar o
  mouse); o rótulo sai da área de arrasto, o resto da janela continua
  arrastável.
- Clique → campo de texto no lugar do rótulo. Enter salva, Esc cancela, nome
  vazio cancela.
- Ao salvar, todas as falas do grupo na tela mostram o nome.
- Visual da identidade GOW: "VOCÊ" em amarelo; participantes (com ou sem
  nome) em cinza claro, Rajdhani caixa alta. Sem cor por pessoa.

### Janela principal

- Nova seção "VOZES GUARDADAS", abaixo de "REUNIÕES": nome, última vez vista,
  número de falas reconhecidas. Ações: Renomear, Apagar (com confirmação),
  Apagar todas (com confirmação).
- Texto: "Vozes ficam só neste computador. Apagar remove a impressão de voz;
  o consentimento é responsabilidade da gow."

### Contrato com o backend

- Comandos: `rename_voice {voice, name}` (durante a sessão),
  `list_voices`, `rename_saved_voice {id, name}`, `delete_voice {id}`,
  `delete_all_voices`.
- Evento: `speaker {voice, label}` para as duas janelas.
- `Line.speaker` ganha `voice?: string` no TypeScript.

## 7. Tratamento de falhas

| Falha | Comportamento |
|---|---|
| Modelo de voz ausente ou falha ao carregar | Sessão segue sem identificação (todos "Participante", como na etapa 1) e um aviso aparece uma vez |
| Erro ao calcular impressão de uma fala | A fala herda o último rótulo; contador no log |
| `voices.json` corrompido | Biblioteca vazia, arquivo original preservado, aviso |
| Falha ao gravar `speakers.json` ou `voices.json` | Nome mantido em memória e na tela; nova tentativa na próxima mudança e no fim da sessão; aviso |
| Renomear sem sessão ativa ou voz inexistente | Erro claro para a UI, sem efeito |

## 8. Testes e validação

### Automáticos (sem dados reais, impressões falsas)

- `SpeakerTracker`: reconhece voz guardada acima do limite; cria grupo
  abaixo; junta falas parecidas e atualiza centroide; fala curta herda o
  último falante; renomear grupo reconhecido como outra voz não altera a voz
  original; média ponderada da biblioteca.
- Sessão: `speakers.json` atômico; Markdown usa os nomes de `speakers.json`,
  inclusive para falas anteriores; compatibilidade com sessões antigas;
  recuperação usa os nomes.
- `VoiceLibrary`: salvar, renomear, apagar, apagar todas; arquivo corrompido;
  impressão de voz nunca em logs nem em arquivos da sessão.
- Pipeline: fala final do sistema já sai com `voice` e rótulo; microfone
  continua "Você"; renomear emite `speaker`.
- UI (vitest): evento `speaker` atualiza todas as falas do grupo; campo de
  nome com Enter e Esc.

### Integração (manual, como o teste do Whisper)

Frases fictícias geradas com `say` em 3 ou 4 vozes do macOS, alternando
falantes, passadas pelo pipeline: cada voz num grupo próprio; depois de
nomeada, reconhecida numa segunda execução.

### CI

O build com ONNX Runtime precisa ser verificado no CI do Windows (risco de
conflito de CRT, como o CTranslate2).

### Validação manual

`docs/validacao-falantes.md`, só com números: reunião real com ≥ 3
participantes remotos; % de falas ≥ 1 s com rótulo certo (meta ≥ 85%);
quantas vezes uma pessoa virou dois grupos e duas viraram um; reconhecimento
numa segunda reunião; CPU e atraso.

## 9. Ordem de entrega

1. Escolha do modelo de voz (licença, tamanho, tempo, qualidade) e manifesto.
2. `SpeakerEmbedder` real + teste de integração com vozes do `say`.
3. `VoiceLibrary` e `SpeakerTracker`.
4. Formato da sessão (`voice`, `speakers.json`, Markdown, recuperação).
5. Integração no pipeline.
6. Comandos e eventos no app Tauri.
7. Interface: renomear na janela flutuante e seção "Vozes guardadas".
8. Validação manual no Mac; depois, verificação no Windows junto com as
   tarefas pendentes da etapa 1.

## 10. Riscos

| Risco | Mitigação |
|---|---|
| Qualidade baixa em falas curtas ou áudio comprimido do Teams | Duração mínima e herança do último falante; limites configuráveis calibrados na validação |
| Duas pessoas no mesmo grupo (vozes parecidas) | Limite de grupo calibrado; sem correção fala a fala nesta etapa (fora do escopo) |
| Reconhecimento errado de voz guardada | Renomear na reunião não altera a voz original; tela de vozes para apagar |
| Troca de falante sem pausa dentro de uma fala | Aceito nesta etapa; corte por troca de voz pode vir depois |
| ONNX Runtime no Windows (CRT, como o CTranslate2) | Checagem no CI do Windows desde a primeira tarefa |
| Dado biométrico (LGPD) | Consentimento pela gow; armazenamento só local; exclusão pela tela |
