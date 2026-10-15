#!/usr/bin/env python3
"""Quantize a sherpa Kokoro v1.0 bundle and compare George on the native CPU runtime.

Requires onnx, onnxruntime, sherpa-onnx==1.13.8, numpy and soundfile.
The model directory must contain the extracted kokoro-multi-lang-v1_0 bundle.
Each benchmark runs in a fresh process; peak RSS includes Python and native libraries.

Example (from the repository root, after installing quantization-requirements.txt):
    python tts-demo/quantize-george.py --bundle /path/to/kokoro-multi-lang-v1_0 \
        --quantized .test-tmp/kokoro-quantization/model.int8.onnx

Set TMPDIR to a scratch directory with at least 1 GB free for ONNX intermediates.
The experiment also needs ffmpeg on PATH to create loudness-matched MP3 samples.
"""
import argparse
import hashlib
import html
import json
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def process_memory():
    fields = {}
    for line in Path("/proc/self/status").read_text().splitlines():
        if line.startswith(("VmRSS:", "VmHWM:")):
            key, value, _ = line.split()
            fields[key.rstrip(":")] = int(value)
    return fields


def worker(args):
    import numpy as np
    import sherpa_onnx
    import soundfile as sf

    root = args.bundle
    text = args.passage.read_text().strip()
    memory_phases = {"before_load": process_memory()}
    start = time.perf_counter()
    config = sherpa_onnx.OfflineTtsConfig(
        model=sherpa_onnx.OfflineTtsModelConfig(
            kokoro=sherpa_onnx.OfflineTtsKokoroModelConfig(
                model=str(args.model), voices=str(root / "voices.bin"),
                tokens=str(root / "tokens.txt"), data_dir=str(root / "espeak-ng-data"),
                # eSpeak's base English voice is British; this bundle has no en-gb alias.
                lexicon=str(root / "lexicon-gb-en.txt"), lang="en",
            ),
            num_threads=args.threads, provider="cpu", debug=False,
        ),
        max_num_sentences=1,
    )
    if not config.validate():
        raise RuntimeError("Invalid native speech configuration")
    tts = sherpa_onnx.OfflineTts(config)
    load_seconds = time.perf_counter() - start
    memory_phases["after_load"] = process_memory()
    speaker_id = args.speaker_id
    start = time.perf_counter()
    opening = tts.generate("On 14 October, Dr. Andersson opened the old library at half past eight.", sid=speaker_id, speed=1.0)
    opening_seconds = time.perf_counter() - start
    if not len(opening.samples):
        raise RuntimeError("The opening sentence produced no speech")
    del opening
    memory_phases["after_opening_sentence"] = process_memory()
    elapsed = []
    cpu = []
    durations = []
    for run in range(args.repeats + 1):
        start = time.perf_counter()
        cpu_start = time.process_time()
        audio = tts.generate(text, sid=speaker_id, speed=1.0)
        seconds = time.perf_counter() - start
        cpu_seconds = time.process_time() - cpu_start
        samples = np.asarray(audio.samples)
        if not len(samples) or not np.isfinite(samples).all() or np.max(np.abs(samples)) < 1e-5:
            raise RuntimeError("Speech output is empty, non-finite, or silent")
        print(f"  {args.label}, {args.threads} threads, run {run}: {seconds:.2f} s for {len(samples) / audio.sample_rate:.2f} s of speech", flush=True)
        if run == 0:
            first_full_seconds = seconds
        else:
            elapsed.append(seconds)
            cpu.append(cpu_seconds)
            durations.append(len(samples) / audio.sample_rate)
        memory_phases[f"after_run_{run}"] = process_memory()
    filename = f"george-{args.label}-{args.threads}threads.wav"
    sf.write(args.output / filename, samples, audio.sample_rate, subtype="PCM_16")
    result = {
        "variant": args.label, "threads": args.threads, "speaker": "bm_george", "speaker_id": speaker_id,
        "model_bytes": args.model.stat().st_size, "load_seconds": load_seconds,
        "opening_sentence_seconds": opening_seconds, "first_full_passage_seconds": first_full_seconds,
        "warm_generation_seconds": elapsed, "median_seconds": statistics.median(elapsed),
        "median_cpu_seconds": statistics.median(cpu), "audio_seconds": statistics.median(durations),
        "times_realtime": statistics.median(durations) / statistics.median(elapsed),
        # ru_maxrss can retain the large quantizer parent's RSS across exec.
        # /proc counters belong to the new executable's address space.
        "peak_process_ram_mib": max(phase["VmHWM"] for phase in memory_phases.values()) / 1024,
        "memory_metric": "/proc/self/status VmHWM", "peak_rss_measurement_valid": True,
        "memory_phases_kib": memory_phases,
        "sample_rate": audio.sample_rate, "file": filename,
        "peak_amplitude": float(np.max(np.abs(samples))), "rms": float(np.sqrt(np.mean(samples ** 2))),
    }
    (args.output / f"{args.label}-{args.threads}threads.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result), flush=True)


def rust_comparison_section(output):
    rows = []
    names = {"alba": "Piper Alba", "fp32": "Kokoro George FP32", "int8": "Kokoro George INT8"}
    for variant, name in names.items():
        filename = f"rust-{variant}.json"
        path = output / filename
        if not path.is_file():
            continue
        result = json.loads(path.read_text())
        phases = {phase["phase"]: phase for phase in result["phases"]}
        peak = max(phase["peak_rss_kib"] for phase in result["phases"])
        values = (
            f'<a href="{filename}">{name}</a>', result["threads"],
            f'{phases["after_load"]["rss_kib"] / 1024:.0f} MiB',
            f'{result["after_synthesis"]["rss_kib"] / 1024:.0f} MiB', f'{peak / 1024:.0f} MiB',
            f'{phases["two_seconds_after_unload"]["rss_kib"] / 1024:.0f} MiB',
            f'{result["median_seconds"]:.2f} s', f'{result["times_realtime"]:.1f}×',
        )
        rows.append("<tr>" + "".join(f"<td>{value}</td>" for value in values) + "</tr>")
    if not rows:
        return ""
    return '''<h2>Rust comparison with Alba</h2><div class="scroll"><table><thead><tr>
<th>Voice</th><th>Threads</th><th>After loading</th><th>After synthesis</th><th>Peak RAM</th><th>After unloading</th><th>Generation</th><th>Real-time speed</th>
</tr></thead><tbody>''' + "".join(rows) + '''</tbody></table></div>
<p>Each voice runs in a fresh Rust release process using the reader's pinned Sherpa-ONNX dependency and static native libraries. The passage, thread count and generation settings match. Generation is the median of three warm runs; voice durations differ. RAM comes from Linux /proc/self/status, including the highest observed VmHWM. The reader UI and audio device are excluded. Model-specific Alba settings match the reader loader.</p>'''


def comparison_page(output, report):
    rows = []
    cards = []
    for result in report["results"]:
        peak = f'{result["peak_process_ram_mib"]:.0f} MiB' if result.get("peak_rss_measurement_valid", True) else "Superseded"
        rows.append("<tr>" + "".join(f"<td>{cell}</td>" for cell in (
            result["variant"].upper(), result["threads"], f'{result["model_bytes"] / 1e6:.1f} MB',
            f'{result["median_seconds"]:.2f} s', f'{result["times_realtime"]:.1f}×',
            f'{result["median_cpu_seconds"]:.2f} s', peak, f'{result["load_seconds"]:.2f} s',
            f'{result["opening_sentence_seconds"]:.2f} s',
        )) + "</tr>")
        if result["threads"] == max(r["threads"] for r in report["results"] if r["variant"] == result["variant"]):
            raw_wav = result["file"]
            wav = f"{Path(raw_wav).stem}-listen.wav"
            mp3 = str(Path(raw_wav).with_suffix(".mp3"))
            subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(output / raw_wav),
                            "-af", "loudnorm=I=-18:TP=-1.5:LRA=11", "-ar", "24000", "-c:a", "pcm_s16le", str(output / wav)], check=True)
            subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(output / wav),
                            "-b:a", "192k", str(output / mp3)], check=True)
            cards.append(f'<article><h2>George · {result["variant"].upper()}</h2>'
                         f'<p>British English · Native sherpa-onnx · Loudness matched</p>'
                         f'<audio controls preload="metadata"><source src="{mp3}" type="audio/mpeg">'
                         f'<source src="{wav}" type="audio/wav"></audio>'
                         f'<p><a href="{wav}">Download WAV</a> · <a href="{mp3}">Download MP3</a></p></article>')
    page = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>George: FP32 versus INT8</title><style>
body{font:17px/1.6 system-ui,sans-serif;background:#f7f5ef;color:#272923;max-width:1050px;margin:48px auto;padding:0 24px}
h1{font-size:38px;line-height:1.2}a{color:#345c40}.cards{display:flex;gap:24px;flex-wrap:wrap}article{background:white;padding:24px;border-radius:16px;flex:1;min-width:260px}
audio{width:100%}table{border-collapse:collapse;width:100%;white-space:nowrap}td,th{text-align:left;padding:10px;border-bottom:1px solid #d7d9ce}.scroll{overflow:auto}blockquote{margin:24px 0;padding:24px;background:#eeeee5}
</style><a href="../index.html">← All voice comparisons</a><h1>George: FP32 versus INT8</h1>
<p>The same Kokoro v1.0 George voice and passage, using the same native speech runtime and British pronunciation data. INT8 is quantized locally from this FP32 model.</p>
<p>INT8 uses Sherpa's upstream dynamic quantization recipe. INT8-MATMUL is a conservative variant that quantizes only matrix operations and keeps convolutions in FP32.</p>
<div class="cards">CARDS</div>RUST_RESULTS<h2>Earlier Python-hosted runs</h2><div class="scroll"><table><thead><tr>
<th>Model</th><th>Threads</th><th>Model size</th><th>Generation</th><th>Real-time speed</th><th>CPU time</th><th>Peak process RAM</th><th>Load</th><th>First sentence</th>
</tr></thead><tbody>ROWS</tbody></table></div>
<p>Generation is the median of REPEATS warm runs. CPU time sums work across inference threads. Model loading is measured separately. First sentence is measured after loading; it is not playback startup. RAM includes Python and native inference allocations, not the reader app. Other Rust builds and desktop activity ran during these measurements, so timing comparisons are provisional. Quantization does not guarantee identical voice quality or a speed improvement.</p>
MEMORY_CORRECTION
<p><a href="results.json">Full results and model hashes</a></p><blockquote>PASSAGE</blockquote>
<script>document.querySelectorAll('audio').forEach(a=>a.addEventListener('play',()=>document.querySelectorAll('audio').forEach(b=>{if(a!==b)b.pause()})))</script></html>'''
    page = page.replace("CARDS", "".join(cards)).replace("ROWS", "".join(rows)).replace("REPEATS", str(report["repeats"]))
    page = page.replace("RUST_RESULTS", rust_comparison_section(output))
    correction = ("<p><strong>Memory measurement correction:</strong> The earlier Python harness used ru_maxrss, which can retain the quantizer launcher's RSS from before exec. Its historical RAM values are superseded by the fresh Rust /proc measurements above. The Python harness now uses /proc counters for future runs.</p>" if any(not result.get("peak_rss_measurement_valid", True) for result in report["results"]) else "")
    page = page.replace("MEMORY_CORRECTION", correction)
    page = page.replace("PASSAGE", html.escape(report["passage"]))
    (output / "index.html").write_text(page)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--quantized", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path(__file__).parent / "george-quantization")
    parser.add_argument("--passage", type=Path, default=Path(__file__).parent / "passage.txt")
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--skip-quantization", action="store_true", help="Benchmark an existing quantized model from this experiment")
    parser.add_argument("--method", choices=("upstream", "matmul"), default="upstream")
    parser.add_argument("--thread-counts", nargs="+", type=int, default=[2, 4])
    parser.add_argument("--reuse-baselines", action="store_true", help="Reuse FP32 result files in --output")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--model", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--label", help=argparse.SUPPRESS)
    parser.add_argument("--threads", type=int, default=2, help=argparse.SUPPRESS)
    parser.add_argument("--speaker-id", type=int, default=26, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error("--repeats must be at least 1")
    if any(count < 1 for count in args.thread_counts):
        parser.error("--thread-counts must be positive")
    args.output.mkdir(parents=True, exist_ok=True)
    if args.worker:
        worker(args)
        return
    from onnxruntime.quantization import QuantType, quantize_dynamic
    import onnx
    original = args.bundle / "model.onnx"
    # Verify the package-specific speaker ID outside the measured workers.
    graph = onnx.load(str(original))
    metadata = {entry.key: entry.value for entry in graph.metadata_props}
    speaker_id = metadata["speaker_names"].split(",").index("bm_george")
    del graph
    previous_path = args.output / "results.json"
    previous = json.loads(previous_path.read_text()) if args.reuse_baselines and previous_path.exists() else {}
    if previous and previous["model_sha256"]["fp32"] != digest(original):
        raise ValueError("Existing benchmark results belong to a different FP32 model")
    args.quantized.parent.mkdir(parents=True, exist_ok=True)
    if args.quantized.exists() and not args.skip_quantization:
        raise FileExistsError(f"Refusing to overwrite an existing model: {args.quantized}")
    if not args.skip_quantization:
        if args.method == "upstream":
            print("Quantizing with the upstream Kokoro v1.0 method (dynamic QUInt8 weights)", flush=True)
            quantize_dynamic(str(original), str(args.quantized), weight_type=QuantType.QUInt8)
        else:
            print("Quantizing MatMul/Gemm only (per-channel QInt8 weights)", flush=True)
            quantize_dynamic(str(original), str(args.quantized), op_types_to_quantize=["MatMul", "Gemm"], per_channel=True, weight_type=QuantType.QInt8)
    print(f"Model sizes: FP32 {original.stat().st_size / 1e6:.1f} MB; INT8 {args.quantized.stat().st_size / 1e6:.1f} MB", flush=True)
    results = []
    candidate_label = "int8" if args.method == "upstream" else "int8-matmul"
    for threads in args.thread_counts:
        for label, model in (("fp32", original), (candidate_label, args.quantized)):
            if label == "fp32" and args.reuse_baselines:
                results.append(json.loads((args.output / f"{label}-{threads}threads.json").read_text()))
                continue
            print(f"Benchmarking {label}, {threads} threads", flush=True)
            subprocess.run([sys.executable, str(Path(__file__).resolve()), "--worker", "--bundle", str(args.bundle),
                            "--quantized", str(args.quantized), "--output", str(args.output), "--passage", str(args.passage),
                            "--repeats", str(args.repeats), "--model", str(model), "--label", label, "--threads", str(threads),
                            "--speaker-id", str(speaker_id)], check=True)
            results.append(json.loads((args.output / f"{label}-{threads}threads.json").read_text()))
    import onnxruntime
    import sherpa_onnx
    report = {
        "date": time.strftime("%Y-%m-%d"), "platform": platform.platform(),
        "cpu": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")), "unknown"),
        "sherpa_onnx": sherpa_onnx.__version__, "onnxruntime_quantizer": onnxruntime.__version__,
        "method": ("onnxruntime.quantization.quantize_dynamic; weight_type=QUInt8; upstream Kokoro v1.0 recipe" if args.method == "upstream" else "onnxruntime.quantization.quantize_dynamic; MatMul/Gemm only; per_channel=True; weight_type=QInt8"),
        "upstream_recipe": "https://github.com/k2-fsa/sherpa-onnx/blob/master/scripts/kokoro/v1.0/dynamic_quantization.py",
        "limitations": ["Python host driving the native sherpa-onnx core, not the reader application", "Timing depends on background load; other Rust builds and desktop activity ran during this experiment", "Perceptual voice quality requires listening to the samples"],
        "model_sha256": {"fp32": digest(original), candidate_label: digest(args.quantized)},
        "repeats": args.repeats, "passage": args.passage.read_text().strip(), "results": results,
    }
    if previous:
        if previous["passage"] != report["passage"] or previous["repeats"] != args.repeats:
            raise ValueError("Existing results use a different passage or repeat count")
        combined = {(item["variant"], item["threads"]): item for item in previous["results"] + results}
        report["results"] = sorted(combined.values(), key=lambda item: (item["threads"], item["variant"]))
        report["model_sha256"] = previous["model_sha256"] | report["model_sha256"]
    report["methods"] = previous.get("methods", {"int8": previous["method"]} if previous else {}) | {candidate_label: report["method"]}
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    comparison_page(args.output, report)
    print(f"Comparison ready: {args.output / 'index.html'}", flush=True)


if __name__ == "__main__":
    main()
