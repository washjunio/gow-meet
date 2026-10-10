"""Compara modelos de impressão de voz com as falas fictícias do `say`.

Uso (só para a escolha do modelo; nada disto vai para o app):
  uv run --no-project --with onnxruntime==1.20.1 --with kaldi-native-fbank==1.20.0 --with numpy \
    python -I scripts/measure-voice-models.py target/voice-models/*.onnx target/voice-fixtures

Para cada modelo: dimensão, tempo por fala (mediana/p90, CPU, 1 thread), semelhança
entre falas da mesma voz (intra) e de vozes diferentes (inter), margem = intra mín − inter
máx, e o agrupamento em tempo real (mesma regra do SpeakerTracker) com limites 0,60/0,65/0,70.
"""
import glob, itertools, os, sys, time, wave

import kaldi_native_fbank as knf
import numpy as np
import onnxruntime as ort


def read(path):
    with wave.open(path) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1, path
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32)


def fbank(pcm_i16):
    # Igual ao WeSpeaker na inferência: escala int16, sem dither, janela hamming, 80 filtros, CMN.
    o = knf.FbankOptions()
    o.frame_opts.dither = 0.0
    o.frame_opts.window_type = "hamming"
    o.mel_opts.num_bins = 80
    f = knf.OnlineFbank(o)
    f.accept_waveform(16000, pcm_i16.tolist())
    f.input_finished()
    m = np.stack([f.get_frame(i) for i in range(f.num_frames_ready)])
    return (m - m.mean(axis=0)).astype(np.float32)


def cluster(order, embs, thr):
    groups = []  # [soma, {voz: falas}]
    for p in order:
        e, who = embs[p], os.path.basename(p).rsplit("_", 1)[0]
        sims = [g[0] @ e / np.linalg.norm(g[0]) for g in groups]
        if sims and max(sims) >= thr:
            g = groups[int(np.argmax(sims))]
            g[0] += e
            g[1][who] = g[1].get(who, 0) + 1
        else:
            groups.append([e.copy(), {who: 1}])
    pure = sum(max(g[1].values()) for g in groups)
    return len(groups), pure / len(order)


models, wav_dir = sys.argv[1:-1], sys.argv[-1]
wavs = sorted(glob.glob(os.path.join(wav_dir, "*.wav")))
voices = sorted({os.path.basename(p).rsplit("_", 1)[0] for p in wavs})
n = max(int(os.path.basename(p).rsplit("_", 1)[1][:-4]) for p in wavs) + 1
order = [os.path.join(wav_dir, f"{v}_{i}.wav") for i in range(n) for v in voices]
feats = {p: fbank(read(p)) for p in wavs}
print(f"{len(voices)} vozes, {len(wavs)} falas")
for mp in models:
    so = ort.SessionOptions()
    so.intra_op_num_threads = 1
    s = ort.InferenceSession(mp, so, providers=["CPUExecutionProvider"])
    embs, times = {}, []
    for p, f in feats.items():
        t = time.perf_counter()
        e = s.run(None, {"feats": f[None]})[0][0]
        times.append((time.perf_counter() - t) * 1000)
        embs[p] = e / np.linalg.norm(e)
    spk = lambda p: os.path.basename(p).rsplit("_", 1)[0]
    intra, inter = [], []
    for a, b in itertools.combinations(wavs, 2):
        (intra if spk(a) == spk(b) else inter).append(float(embs[a] @ embs[b]))
    groups = " ".join(f"{thr:.2f}→{g} grupos/{pur:.0%} puro" for thr in (0.60, 0.65, 0.70)
                      for g, pur in [cluster(order, embs, thr)])
    print(f"{os.path.basename(mp)} ({os.path.getsize(mp) / 1e6:.1f} MB): dim={len(embs[wavs[0]])} | "
          f"ms/fala mediana={np.median(times):.0f} p90={np.percentile(times, 90):.0f} | "
          f"intra mín={min(intra):.3f} | inter máx={max(inter):.3f} | margem={min(intra) - max(inter):+.3f} | {groups}")
