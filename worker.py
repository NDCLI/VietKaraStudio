"""Isolated, cancellable speech worker. This imports open-source models, never the VCL executable."""
from __future__ import annotations

import json
from pathlib import Path
import sys


def progress(value, message):
    print("OBS_PROGRESS " + json.dumps({"progress": value, "message": message}, ensure_ascii=False), flush=True)


def write_result(path, result):
    """Model outputs may contain NumPy scalars, including numpy.bool_."""
    import numpy as np
    def native(value):
        if isinstance(value, np.generic):
            return value.item()
        raise TypeError(f"Không thể lưu kiểu {type(value).__name__}")
    destination = Path(path)
    content = json.dumps(result, ensure_ascii=False, allow_nan=False, default=native)
    temporary = destination.with_suffix(".tmp")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(destination)


def vocal_bounds(samples, sample_rate=16000):
    """Trim sustained near-silence, without using a speech VAD on singing."""
    import numpy as np
    frame=max(1,round(sample_rate*.02))
    count=len(samples)//frame
    if not count:return 0,len(samples)
    rms=np.sqrt(np.mean(samples[:count*frame].reshape(count,frame).astype(np.float64)**2,axis=1))
    high=float(np.percentile(rms,95))
    low=float(np.percentile(rms,10))
    if high<1e-5 or high<low*5:return 0,len(samples)
    active=rms>max(1e-5,high*.025)
    sustained=np.convolve(active.astype(int),np.ones(5,dtype=int),'same')>=4
    indices=np.flatnonzero(sustained)
    if not len(indices):return 0,len(samples)
    start=max(0,int(indices[0]*frame-sample_rate*.5))
    end=min(len(samples),int((indices[-1]+1)*frame+sample_rate*.7))
    return start,end


def check_alignment(words, supplied_lyrics):
    short=sum(w['end']-w['start']<.05 for w in words)
    first=words[:40]
    first_short=sum(w['end']-w['start']<.05 for w in first)
    if supplied_lyrics and len(words)>=12 and (short/len(words)>.35 or first_short/len(first)>.6):
        raise RuntimeError(f"Căn lời chưa tin cậy: {short}/{len(words)} từ bị dồn vào mốc quá ngắn. Kết quả cũ được giữ nguyên. Hãy kiểm tra lời đúng bản hát hoặc căn từng đoạn ngắn.")
    return short


def repair_prefix(model, samples, words, offset, language):
    """Repair collapsed early tokens only when a local ASR phrase agrees with the supplied lyrics."""
    import difflib
    import re
    import unicodedata
    prefix=words[:40]
    if not any(w['end']-w['start']<.05 for w in prefix):return 0
    segments,_=model.transcribe_original(samples[:16000*30],language=language or 'vi',
                                         word_timestamps=True,beam_size=5,vad_filter=False,
                                         condition_on_previous_text=False,temperature=0.)
    heard=[]
    for segment in segments:
        for item in segment.words or []:
            tokens=item.word.split()
            for j,token in enumerate(tokens):
                heard.append({'text':token,'start':offset+item.start+(item.end-item.start)*j/max(1,len(tokens)),
                              'end':offset+item.start+(item.end-item.start)*(j+1)/max(1,len(tokens))})
    def key(text):return re.sub(r'[^\w]','',unicodedata.normalize('NFC',text).casefold())
    matcher=difflib.SequenceMatcher(None,[key(w['text']) for w in prefix],[key(w['text']) for w in heard],autojunk=False)
    repaired=0
    for block in matcher.get_matching_blocks():
        if block.size<3:continue
        for j in range(block.size):
            i=block.a+j;word=words[i];anchor=heard[block.b+j]
            if word['end']-word['start']>=.05 or abs(anchor['start']-word['start'])>2:continue
            start=max(offset,anchor['start'],words[i-1]['end'] if i else offset)
            end=min(anchor['end'],words[i+1]['start'] if i+1<len(words) else anchor['end'])
            if end-start>=.08:
                word.update(start=float(start),end=float(end),estimated=True)
                repaired+=1
    return repaired


def run(request, device):
    from hardware import prepare_cuda
    prepare_cuda()
    import stable_whisper
    import math
    import soundfile as sf
    import numpy as np
    from scipy.signal import resample_poly
    samples, sample_rate = sf.read(request["vocals"], dtype="float32", always_2d=True)
    samples = samples.mean(axis=1)
    if sample_rate != 16000:
        divisor = math.gcd(sample_rate, 16000)
        samples = resample_poly(samples, 16000 // divisor, sample_rate // divisor).astype(np.float32)
    compute_type = request.get("compute_type") or ("float16" if device == "cuda" else "int8")
    device_index = int(request.get("device_index", 0))
    from model_manager import resolve_whisper
    local = resolve_whisper(request.get('models_root', Path(__file__).parent / 'data'), request['model'])
    model = stable_whisper.load_faster_whisper(str(local) if local else request["model"], device=device, device_index=device_index,
                                             compute_type=compute_type)
    language = request["language"] if request["language"] != "auto" else None
    words, texts = [], []
    lyrics = request.get("lyrics", "").strip()
    trim_offset=0.
    repaired_prefix=0
    label = request.get("name") or device
    progress(50, f"Đang {'căn lời' if lyrics else 'nhận dạng'} bằng Whisper {request['model']} · {label} ({compute_type})…")
    if lyrics:
        import unicodedata
        first,last=vocal_bounds(samples)
        trim_offset=first/16000
        aligned_samples=samples[first:last]
        progress(51,f"Đang căn lời từ vùng vocal {trim_offset:.2f}s · {label}…")
        result = model.align(aligned_samples, unicodedata.normalize('NFC',lyrics).lower(), language=language or "vi",
                             original_split=True, vad=False, verbose=False).to_dict()
        segments = result.get("segments", [])
        raw_lines = [line.split() for line in lyrics.splitlines() if line.strip()]
        for li, tokens in enumerate(raw_lines):
            if li >= len(segments):
                raise RuntimeError("Model không căn đủ dòng lời. Thử chia lời thành các câu ngắn hơn.")
            segment = segments[li]
            heard = segment.get("words", [])
            flat = []
            for item in heard:
                tokens_heard = item.get("word", "").split()
                for k, token in enumerate(tokens_heard):
                    a, b = float(item["start"]), float(item["end"])
                    flat.append({"start": a + (b-a)*k/max(1,len(tokens_heard)),
                                 "end": a + (b-a)*(k+1)/max(1,len(tokens_heard))})
            if len(flat) == len(tokens):
                spans = flat
            else:
                a, b = float(segment["start"]), float(segment["end"])
                if b <= a:
                    raise RuntimeError(f"Không tìm được thời gian của dòng {li + 1}.")
                sizes = [max(1,len(t)) for t in tokens]
                spans, t = [], a
                for size in sizes:
                    end = t + (b-a)*size/sum(sizes)
                    spans.append({"start": t, "end": end})
                    t = end
            for token, span in zip(tokens, spans):
                start = max(0., span["start"]+trim_offset)
                words.append({"text": token, "start": start, "end": max(start+.03, span["end"]+trim_offset),
                              "line": li, "role": "all", "estimated": bool(len(flat) != len(tokens) or span["end"]-span["start"] < .05)})
            texts.append(" ".join(tokens))
    else:
        segments, info = model.transcribe_original(samples, language=language,
                                                   word_timestamps=True, beam_size=5, vad_filter=False,
                                                   condition_on_previous_text=False, temperature=0.)
        for segment in segments:
            li = len(texts)
            tokens_in_line = []
            for item in segment.words or []:
                tokens = item.word.strip().split()
                for k, token in enumerate(tokens):
                    start = max(0., item.start + (item.end-item.start)*k/max(1,len(tokens)))
                    end = item.start + (item.end-item.start)*(k+1)/max(1,len(tokens))
                    words.append({"text": token, "start": start, "end": max(start+.03,end),
                                  "line": li, "role": "all", "estimated": bool(end-start < .05 or item.probability < .35)})
                    tokens_in_line.append(token)
            if tokens_in_line:
                texts.append(" ".join(tokens_in_line))
            progress(min(92, 50 + segment.end / max(info.duration, 1) * 42), f"Whisper · {label}: đang nhận dạng từng từ…")
    if not words:
        raise RuntimeError("Không nhận được lời. Hãy kiểm tra vocal hoặc dán lời có sẵn để căn lại.")
    if lyrics:
        try:
            repaired_prefix=repair_prefix(model,aligned_samples,words,trim_offset,language)
        except Exception as exc:
            # This is an optional rescue; the primary alignment must still pass quality checks.
            progress(94,'Giữ mốc căn lời chính; chưa đối chiếu được phần đầu: '+str(exc)[:120])
    short=check_alignment(words,bool(lyrics))
    for i, word in enumerate(words):
        if i and word["start"] < words[i-1]["end"] - .05:
            word["estimated"] = True
            words[i-1]["estimated"] = True
    write_result(request["output"], {"words": words, "text": "\n".join(texts), "device": device,
                                                 "device_index": device_index, "compute_type": compute_type,
                                   "device_name": label, "fallback_reason": request.get("fallback_reason"),
                                   "alignment_diagnostics":{"vocal_start":trim_offset,"short_words":short,
                                                            'repaired_prefix':repaired_prefix}})


if __name__ == "__main__":
    request = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    if "device" not in request:
        from hardware import detect
        request.update(detect()["whisper"])
    run(request, request["device"])
