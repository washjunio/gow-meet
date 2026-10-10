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
