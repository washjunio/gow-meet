#!/usr/bin/env bash
# Gera áudio fictício em inglês com a voz do macOS. Só frases inventadas.
set -euo pipefail
OUT=crates/core/tests/fixtures
mkdir -p "$OUT"
say_wav() { say -v Samantha --data-format=LEI16@16000 -o "$OUT/$1.wav" "$2"; printf '%s\n' "$3" > "$OUT/$1.txt"; }
say_wav s1 "Let's review the project timeline for next week." "Let's review the project timeline for next week."
say_wav s2 "The new dashboard should be ready by Friday." "The new dashboard should be ready by Friday."
say_wav s3 "Can you share the budget estimate with the team?" "Can you share the budget estimate with the team?"
say_wav s4 "We need to decide which features go into the first release." "We need to decide which features go into the first release."
say_wav s5 "I will send a summary after this call." "I will send a summary after this call."
say -v Samantha --data-format=LEI16@16000 -o "$OUT/meeting.wav" \
  "Good morning everyone. [[slnc 1500]] Let's review the project timeline for next week. [[slnc 1500]] \
   The new dashboard should be ready by Friday. [[slnc 1500]] Can you share the budget estimate with the team? \
   [[slnc 1500]] I will send a summary after this call."
ls -la "$OUT"
