# gow-meet — Etapa 3: reuniões em português e troca de idioma

Data: 2026-10-10 · Status: aguardando revisão · Base: etapa 2 (branch `etapa2`)

## 1. Contexto e objetivo

Até a etapa 2 o app só entende reuniões em inglês: transcreve com o Whisper
`small.en` (só inglês) e traduz com o Opus-MT en→pt. A etapa 3 permite usar o
app também em reuniões em **português**, com tradução para o **inglês**, e
escolher o idioma falado:

- **antes de iniciar** cada reunião, num seletor na janela principal;
- **durante a reunião**, trocando de idioma com um clique (ex.: a conversa
  começa em português e passa para o inglês quando um convidado entra).

Nesta etapa os idiomas são só EN e PT-BR, e a tradução é sempre para o outro.

### Critério de sucesso

- Reunião real em português: atraso da fala até a tradução com p90 ≤ 3 s no
  M2 (mesma meta da etapa 1) e tradução para o inglês julgada útil pelo
  usuário.
- Reuniões em inglês sem piora: mesmo modelo, mesmo atraso da etapa 1.
- Troca de idioma com o outro par já carregado: a primeira fala depois da
  troca já sai no idioma novo, sem pausa perceptível e sem perder fala.
- Nenhuma fala sai com transcrição de um idioma e tradução do outro.
- Identificação de vozes funciona igual nos dois idiomas, e uma voz nomeada
  numa reunião em inglês é reconhecida numa reunião em português.

### Restrições (herdadas)

- Processamento 100% local. Nada de áudio, texto ou impressão de voz sai da
  máquina.
- O áudio nunca é gravado em disco.
- Logs nunca contêm texto de reunião nem nomes de pessoas.
- Modelos com licença que permita uso em empresa, baixados com revisão fixa e
  conferidos por hash.
- Mac primeiro, Windows em seguida.

### Fora do escopo da etapa 3

- Detecção automática do idioma falado.
- Idiomas além de EN e PT-BR, e escolher o idioma de tradução separado do
  idioma falado.
- Idiomas diferentes por fonte (ex.: você fala em português e os outros em
  inglês na mesma reunião).
- Retraduzir falas já feitas depois de uma troca.

## 2. Decisões tomadas

| Tema | Decisão |
|---|---|
| Escolha do idioma | A cada reunião, antes de iniciar; o seletor lembra a última escolha |
| Troca durante a reunião | Permitida, pela janela principal e pela janela flutuante |
| Velocidade da troca | Os dois pares de idioma ficam carregados durante a reunião (abordagem 1); o par escolhido carrega primeiro, o outro em segundo plano |
| Transcrição EN | Continua com `small.en` (sem perda) |
| Transcrição PT | Whisper `small` multilíngue, confirmado na medição |
| Tradução PT→EN | Opus-MT pt→en convertido para CTranslate2, escolhido na medição e publicado no repositório `gow-solucoes` do Hugging Face (publicação com OK do usuário) |
| Alternativa descartada | Carregar o outro par só na troca (congela a transcrição por segundos). Modelo multilíngue único para os dois idiomas só volta à mesa se a medição mostrar perda desprezível no EN |

## 3. Arquitetura

### Pares de idioma (`gow-meet-core`)

Um módulo pequeno (`lang.rs`) define o tipo `Lang { En, Pt }` e o par de cada
idioma falado:

| Idioma falado | Transcrição | Tradução | Saída |
|---|---|---|---|
| EN | `whisper-small-en` | `opus-mt-en-pt` com prefixo `>>pob<<` | PT |
| PT | `whisper-small` (multilíngue, idioma `pt`) | `opus-mt-pt-en` (id definitivo na medição) | EN |

### Troca na mesma fila do áudio

- O pipeline ganha `set_language(lang)`. Ela não troca o modelo "por fora":
  primeiro fecha as falas abertas das duas fontes (como o encerramento faz),
  depois coloca um **marcador de idioma** na fila da transcrição.
- A thread de transcrição processa a fila em ordem: o que veio antes do
  marcador sai no idioma antigo, o que veio depois sai no novo. O marcador
  segue para a fila da tradução, que troca de modelo no mesmo ponto.
- `coalesce` trata o marcador como barreira: nunca junta nem descarta jobs de
  lados diferentes dele.
- A thread de transcrição guarda os dois Whisper e a de tradução os dois
  Opus-MT. O marcador só escolhe qual usar.
- Se o par novo ainda não estiver pronto, o app só coloca o marcador quando
  ele estiver (seção 6). Até lá a reunião segue no idioma atual.
- Trocar para o idioma atual não faz nada. Pedir outra troca antes da
  anterior ficar pronta cancela a anterior (vale o último pedido).

### Filtro de alucinações

O filtro do Whisper (hoje só frases em inglês, como "thanks for watching")
passa a ter uma lista por idioma. A lista PT inclui as alucinações conhecidas
do Whisper em português (ex.: "Legendas pela comunidade Amara.org",
"Obrigado por assistir") e é completada na medição.

### Vozes

Nada muda. A identificação roda na thread de transcrição, independe do
idioma e usa a mesma biblioteca de vozes.

## 4. Fluxo de dados

```text
iniciar(lang) → garante e carrega o par de lang → reunião começa
              ↘ (segundo plano) carrega o outro par, se os modelos já estão no disco
áudio → segmentador → fila ASR [jobs… | Marcador(pt) | jobs…]
      → Whisper do idioma corrente → texto + lang → identificação de voz
      → fila tradução [… | Marcador(pt) | …] → Opus-MT do idioma corrente
      → Line{lang} → sessão / UI
UI: troca(lang) → set_language → (prepara o par se preciso) → fecha falas abertas → Marcador
             → evento language{lang, status} → as duas janelas atualizam
```

## 5. Armazenamento

### Sessão

- `lines.jsonl`: `Line` ganha `lang` (`"en"` ou `"pt"`), o idioma falado. A
  tradução é sempre o outro idioma. Sem o campo (sessões antigas), vale o
  `source_lang` de `meta.json`.
- `meta.json`: `source_lang`/`target_lang` continuam com o idioma de início.
  `models` lista os modelos dos dois idiomas quando os dois forem usados
  (chaves `asr`/`translate` para o idioma de início, `asr_pt`/`translate_pt`
  ou `asr_en`/`translate_en` para o outro).
- `transcript.md`: quando o idioma muda entre falas, entra um separador
  ("— agora em português —" / "— agora em inglês —"). Reunião com um idioma
  só fica como hoje.

### Configuração (`config.json`)

- As chaves atuais (`whisper_model`, `opus_model`, `opus_prefix`) continuam
  valendo para o EN; um `config.json` existente não quebra.
- Novas chaves para o PT: `whisper_model_pt`, `opus_model_pt`,
  `opus_prefix_pt` (padrões vindos da medição).
- `source_lang`/`target_lang` deixam de mandar no idioma: o idioma vem do
  seletor. As chaves são aceitas e ignoradas.

### Última escolha

Guardada pela janela principal no armazenamento local da própria janela; sem
ela, o padrão é EN.

### Logs

Só idioma e status da troca ("idioma → pt", "par pt pronto em 2,1 s"). Nunca
texto.

## 6. App (Tauri)

- `start_session { title, lang }`: garante e carrega o par de `lang` (como
  hoje). Depois de a reunião começar, carrega o outro par em segundo plano
  **só se os modelos já estiverem no disco**. Nada é baixado sem pedido de
  troca.
- `set_language { lang }`, novo: durante a reunião.
  - Outro par carregado → `Pipeline::set_language` na hora.
  - Outro par carregando ou faltando no disco → o app baixa (com progresso) e
    carrega em segundo plano, e só então chama `Pipeline::set_language`.
  - Sem reunião ativa → erro claro, sem efeito.
- Evento novo `language { lang, status, progress? }` para as duas janelas:
  `status` é `preparing` (com `progress` de 0 a 1 enquanto baixa), `active`
  ou `failed`.
- `Partial`, `Final` e `Translated` carregam o `lang` da fala.
- Encerrar a reunião durante uma preparação descarta o marcador pendente; o
  download pode terminar e fica guardado para a próxima vez.

## 7. Interface

### Janela principal

- Ao lado de "Iniciar": seletor "Idioma falado: Inglês / Português", com a
  legenda "tradução para português" ou "tradução para inglês".
- Durante a reunião o seletor continua ativo e troca o idioma. Enquanto
  prepara, mostra "Preparando português… 40%".

### Janela flutuante

- Botão pequeno no topo: "EN → PT" ou "PT → EN". Um clique troca. Enquanto
  prepara, mostra o progresso e a reunião segue no idioma atual.
- Na troca, uma linha discreta entre as falas: "— agora em português —".
- Visual GOW: mesmo estilo dos rótulos (Rajdhani, caixa alta, cinza; amarelo
  só no idioma ativo).

### Histórico

- Cada fala mostra o original e a tradução certos, com o separador quando o
  idioma muda.

## 8. Tratamento de falhas

| Falha | Comportamento |
|---|---|
| Download do outro par falha (ex.: sem internet) | Reunião segue no idioma atual; aviso "Não foi possível preparar o português"; o seletor e o botão voltam ao idioma atual (`language{status: failed}`) |
| Outro par baixa mas não carrega | Mesmo comportamento; erro no log sem conteúdo |
| Tradução PT→EN dá erro numa fala | A fala aparece sem tradução, como já acontece no EN |
| Sem GPU | Mesma regra de hoje: usa o processador e avisa uma vez |
| Pouca memória para os dois pares | Aceito nesta etapa (~350 MB a mais no M2); medido na validação |

## 9. Testes e validação

### Medição (primeira tarefa, antes de mexer no app)

- Frases fictícias em português geradas com `say` (voz `Luciana`), como as
  do inglês.
- `small` multilíngue em PT: atraso (p50/p90) e WER no M2. Se reprovar,
  medir `turbo` e `medium`.
- Candidatos pt→en: licença, tamanho, tempo por frase e qualidade em frases
  de reunião.
- Opcional: `small` multilíngue também em EN, para saber se a alternativa de
  modelo único seria viável.
- Resultado em `docs/modelos-pt.md`, só números.

### Automáticos

- Pipeline: áudio antes do marcador sai no idioma antigo e depois no novo;
  as duas fontes trocam juntas; `coalesce` respeita o marcador; troca para o
  mesmo idioma não faz nada; falas abertas fecham na troca.
- Tradução: a fala usa o tradutor do idioma em que foi transcrita.
- Sessão: `lang` em `Line`; sessões antigas sem `lang`; Markdown com
  separador.
- Filtro de alucinações por idioma.
- App: troca sem reunião ativa; falha de download volta ao idioma atual;
  pedido novo cancela o anterior.
- UI (vitest): seletor lembra a última escolha; evento `language` atualiza
  as duas janelas; separador entre falas.

### Integração (manual, como o teste do Whisper)

Áudio fictício com frases em português e inglês, com troca de idioma no
meio, passado pelo pipeline com os modelos reais.

### Validação manual

`docs/validacao-idiomas.md`, só com números: reunião real em português
(atraso, utilidade da tradução, vozes); reunião com troca no meio (falas
perdidas ou com idioma errado na troca); memória e CPU com os dois pares
carregados.

## 10. Ordem de entrega

1. Medição dos modelos PT e escolha do pt→en; manifesto; publicação no
   Hugging Face (com OK do usuário).
2. `lang.rs`, `lang` em `Line`, filtro por idioma, Markdown com separador.
3. Marcador de idioma no pipeline (filas, `coalesce`, dois modelos por
   thread).
4. App: `start_session` com idioma, carregamento em segundo plano,
   `set_language`, evento `language`, configuração.
5. Interface: seletor na janela principal, botão na janela flutuante,
   separadores, histórico.
6. Validação manual no Mac; Windows junto com as pendências das etapas 1 e 2.

## 11. Riscos

| Risco | Mitigação |
|---|---|
| `small` multilíngue lento ou impreciso em PT | Medição antes de tudo; alternativas `turbo` e `medium` |
| Tradução pt→en fraca para conversa | Comparar candidatos na medição; trocar o modelo depois é só manifesto e configuração |
| Fala longa cortada na troca vira duas falas | Aceito: a troca fecha a fala aberta de propósito |
| Pessoa troca de idioma sem clicar | Fora do escopo (sem detecção automática); a tradução daquele trecho fica ruim até a troca |
| Memória com dois Whisper na GPU | Medido na validação; se pesar, carregar o outro par só na troca vira opção na configuração |
