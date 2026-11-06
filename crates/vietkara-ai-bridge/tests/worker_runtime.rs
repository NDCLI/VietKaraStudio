use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;
use vietkara_ai_bridge::{AiError, AiKind, AiRequest, Worker, WorkerConfig, run_request};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn python() -> String {
    std::env::var("VIETKARA_PYTHON").unwrap_or_else(|_| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    })
}

fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

fn musical_fixture() -> Vec<i16> {
    let sample_rate = 11_025_u32;
    let note_samples = (sample_rate as usize * 3) / 10;
    let mut random = 2_301_u32;
    let mut samples = vec![0_i16; note_samples * 160];
    for note in 0..160 {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let pitch = 48 + random % 32;
        let frequency = 440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0);
        let phase = f64::from(random % 10_000) / 10_000.0 * std::f64::consts::TAU;
        let start = note * note_samples;
        for index in 0..note_samples {
            let time = index as f64 / f64::from(sample_rate);
            let envelope = (time / 0.012).min(1.0) * (-time * 0.9).exp();
            let fundamental = (std::f64::consts::TAU * frequency * time + phase).sin();
            let harmonic2 = (2.0 * std::f64::consts::TAU * frequency * time + phase * 0.7).sin();
            let harmonic3 = (3.0 * std::f64::consts::TAU * frequency * time + phase * 0.4).sin();
            let value = 0.22 * envelope * (fundamental + 0.34 * harmonic2 + 0.16 * harmonic3);
            samples[start + index] = (value * f64::from(i16::MAX)) as i16;
        }
    }
    samples
}

fn request(reference: &Path, beat: &Path, cache: &Path, algorithm: &str) -> AiRequest {
    AiRequest::new(
        AiKind::SyncSuggest,
        vietkara_domain::new_id(),
        serde_json::json!({
            "reference": reference,
            "beat": beat,
        }),
        serde_json::json!({
            "device": "cpu",
            "data_root": cache,
            "algorithm_option": algorithm,
        }),
    )
}

fn config(root: &Path) -> WorkerConfig {
    WorkerConfig::new(python())
        .arg(root.join("ai_worker.py").to_string_lossy().into_owned())
        .timeout(Duration::from_secs(120))
        .env("PYTHONIOENCODING", "utf-8")
}

#[test]
fn real_jsonl_worker_syncs_caches_and_cancels_cpu_analysis() {
    let root = workspace_root();
    let temp = tempfile::tempdir().unwrap();
    let reference = temp.path().join("bản hát.wav");
    let beat = temp.path().join("beat có intro.wav");
    let cache = temp.path().join("app-data");
    let reference_samples = musical_fixture();
    let mut beat_samples = vec![0_i16; 6 * 11_025];
    beat_samples.extend_from_slice(&reference_samples);
    write_wav(&reference, &reference_samples, 11_025);
    write_wav(&beat, &beat_samples, 11_025);

    let first = request(&reference, &beat, &cache, "fixture-v1");
    let mut worker = Worker::spawn(config(&root), first.clone()).unwrap();
    let mut progress = Vec::new();
    let result = worker
        .run(|value, message| progress.push((value, message.to_owned())))
        .unwrap();
    assert!(!progress.is_empty(), "real worker should report progress");
    assert!(progress.iter().any(|(value, _)| *value >= 18.0));
    assert_eq!(result["requires_confirmation"], true);
    let offset = result["offset_seconds"].as_f64().unwrap();
    assert!((offset - 6.0).abs() < 0.3, "detected offset {offset}");

    let cached_request = request(&reference, &beat, &cache, "fixture-v1");
    let mut cached_worker = Worker::spawn(config(&root), cached_request).unwrap();
    let mut cached_progress = Vec::new();
    let cached_result = cached_worker
        .run(|value, message| cached_progress.push((value, message.to_owned())))
        .unwrap();
    assert_eq!(cached_result, result);
    assert!(cached_progress.is_empty(), "cache hit should skip analysis");

    let mut changed_beat = vec![0_i16; 11_025];
    changed_beat.extend_from_slice(&beat_samples);
    write_wav(&beat, &changed_beat, 11_025);
    let mut changed_worker = Worker::spawn(
        config(&root),
        request(&reference, &beat, &cache, "fixture-v1"),
    )
    .unwrap();
    let mut changed_progress = Vec::new();
    let changed_result = changed_worker
        .run(|value, message| changed_progress.push((value, message.to_owned())))
        .unwrap();
    assert!(
        !changed_progress.is_empty(),
        "changed media fingerprint must miss cache"
    );
    let changed_offset = changed_result["offset_seconds"].as_f64().unwrap();
    assert!(
        (changed_offset - 7.0).abs() < 0.3,
        "changed offset {changed_offset}"
    );

    let mut cancelled_worker = Worker::spawn(
        config(&root),
        request(&reference, &beat, &cache, "fixture-cancel"),
    )
    .unwrap();
    let cancel = cancelled_worker.cancel_flag();
    let cancelled = cancelled_worker.run(|_, _| cancel.store(true, Ordering::SeqCst));
    assert!(matches!(cancelled, Err(AiError::Cancelled)));
}

#[test]
#[ignore = "requires the separately installed Whisper small model and a speech WAV fixture"]
fn installed_whisper_small_recognizes_speech_through_jsonl_worker() {
    let root = workspace_root();
    let vocals = PathBuf::from(
        std::env::var("VIETKARA_SPEECH_FIXTURE")
            .expect("VIETKARA_SPEECH_FIXTURE must point to a speech WAV"),
    );
    let models_root = PathBuf::from(
        std::env::var("VIETKARA_MODELS_ROOT")
            .expect("VIETKARA_MODELS_ROOT must point to the app data root"),
    );
    let temp = tempfile::tempdir().unwrap();
    let request = AiRequest::new(
        AiKind::Recognize,
        vietkara_domain::new_id(),
        serde_json::json!({ "vocals": vocals }),
        serde_json::json!({
            "device": "cpu",
            "model": "small",
            "language": "en",
            "models_root": models_root,
            "data_root": temp.path().join("app-data"),
        }),
    );
    let mut progress = Vec::new();
    let result = run_request(
        WorkerConfig::new(python())
            .arg(root.join("ai_worker.py").to_string_lossy().into_owned())
            .timeout(Duration::from_secs(5 * 60))
            .env("PYTHONIOENCODING", "utf-8"),
        request,
        |value, message| progress.push((value, message.to_owned())),
    )
    .unwrap();
    assert!(progress.iter().any(|(value, _)| *value >= 50.0));
    let words = result["words"]
        .as_array()
        .expect("worker returns word timings");
    let transcript = words
        .iter()
        .filter_map(|word| word["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(transcript.contains("hello"), "recognized: {transcript}");
    assert!(
        transcript.contains("recognition"),
        "recognized: {transcript}"
    );
    assert!(
        words
            .iter()
            .all(|word| { word["start"].as_f64().is_some() && word["end"].as_f64().is_some() })
    );
}
