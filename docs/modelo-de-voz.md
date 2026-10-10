# Modelo de impressão de voz — escolha (etapa 2)

Data: 2026-10-09 · Máquina: Mac M2 8 GB · Falas: 4 vozes do `say` × 8 frases fictícias (`scripts/gen-voice-fixtures.sh`)

| Modelo (revisão) | Licença | MB | Dim. | p90 ONNX Runtime/Python | p90 tract/Rust (Task 2) | Margem | Grupos a 0,60 / 0,65 / 0,70 |
|---|---|---|---|---|---|---|---|
| wespeaker-ecapa-tdnn512-LM (a2f3dcb) | CC BY 4.0 | 24,9 | 192 | 34 ms | 53 ms | +0,058 | 3 / 4 / 4 |
| wespeaker-voxceleb-campplus-LM (c5e01c6) | Apache-2.0 | 29,3 | 512 | 45 ms | 74 ms | +0,061 | 3 / 4 / 4 |
| wespeaker-voxceleb-resnet34-LM (f0c48c2) | CC BY 4.0 | 26,5 | 256 | 155 ms | 210 ms | +0,040 | 3 / 4 / 4 |

Escolha: wespeaker-ecapa-tdnn512-LM (empate em qualidade com o CAM++, metade do tempo, menor).
Limites padrão: grupo 0,65 (0,60 junta duas vozes parecidas), biblioteca 0,70.
Runtime: tract-onnx 0.23.8 (o ONNX Runtime estático colide com o protobuf do CTranslate2).

CI Windows (commit c794715): tract e rustfft compilam; link segue falhando só pelo ct2rs (LNK2038) — LNK2005/LNK2038 citam apenas libct2rs e libesaxx_rs (esaxx, do sentencepiece), nenhum objeto de tract/rustfft.
