#!/usr/bin/env bash
set -euo pipefail
OUT="${1:-models/opus-mt-en-pt-ct2}"
SRC="${2:-Helsinki-NLP/opus-mt-tc-big-en-pt}"
python3 -m venv .venv-convert
. .venv-convert/bin/activate
pip install "ctranslate2>=4.4" "transformers[sentencepiece]>=4.44" torch
ct2-transformers-converter --model "$SRC" \
  --output_dir "$OUT" --quantization int8 --copy_files source.spm target.spm
ls -la "$OUT"
