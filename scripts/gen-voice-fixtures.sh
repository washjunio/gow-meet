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
