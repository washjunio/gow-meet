# Modelos para reuniões em português (etapa 3)

Máquina: Mac M2 8 GB · Data: 10/10/2026 · Commit: 0cd6e92

## Transcrição (Whisper, idioma pt, frases fictícias da voz Luciana)

| Modelo | Tamanho | WER médio PT | Tempo por frase (s1..s5) | CLI p50 / p90 (fala → tradução) |
|---|---|---|---|---|
| ggml-small-q5_1 (sha256 `ae85e4a9…11bb`) | 190 MB | 0,00 | 1,01 / 0,69 / 0,48 / 0,44 / 0,41 s | 1237 / 1328 ms (com `opus-mt-itc-en`) |

Parcial (transcrição) no CLI: p50 520 ms, p90 607 ms.

EN com o `small` multilíngue: WER 0,00 nas cinco frases, igual ao `small.en`. Um terceiro teste do `asr_whisper` com o `small.en` falhou uma vez numa execução e passou 4/4 ao repetir (intermitente, não reproduzido). Ou seja, o "modelo único" seria viável em precisão para EN, mas a decisão da spec (EN continua com `small.en`) não muda.

## Tradução pt→en (CTranslate2 int8)

| Candidato | Revisão | Licença | Tamanho | Palavras-chave (8) | p50 / p90 por frase |
|---|---|---|---|---|---|
| opus-mt-itc-en | ce67164f | Apache-2.0 | 74 MB | 8/8 | 56 / 98 ms |
| opus-mt-ROMANCE-en | e9ca9975 | Apache-2.0 | 78 MB | 7/8 (falhou "question": "Anybody have any doubts before we follow?") | 64 / 79 ms |
| opus-mt-tc-bible-big-itc-… (`>>eng<<`) | 4bcdcc25 | Apache-2.0 | 220 MB | 8/8 | 135 / 161 ms |

As três conversões funcionaram (arquivos: `model.bin`, `config.json`, `shared_vocabulary.json`, `source.spm`, `target.spm`). O `itc-en` funciona sem prefixo.

## Escolha

- Whisper PT: `whisper-small` (WER 0,00 e p90 1328 ms ≤ 3 s; não foi preciso medir o `medium`).
- pt→en: `opus-mt-itc-en` (`Helsinki-NLP/opus-mt-itc-en`, revisão ce67164f); prefixo: nenhum. Passa nas 8 palavras-chave, p90 98 ms e é o menor em disco (74 MB); o bible-big traduz um pouco mais naturalmente, mas é 3 vezes maior sem ganho que justifique.

## Alucinações

Frases que o Whisper devolveu em silêncio/ruído em português (minúsculas, exatamente como saíram depois de `clean_transcript`), uma por linha; "nenhuma" se não houve.

nenhuma

(Bruto: silêncio → " [Música]", ruído → " [Som de tambor]"; ambos já viram `None` em `clean_transcript`.)
