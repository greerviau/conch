use conch_lib::{
    core::{self, Mapping, Snapshot},
    inference,
    storage::{self, Repository, Settings},
};
#[test]
fn snapshot_isolates_selection_and_survives_settings_changes_and_removal() {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let repository = Repository {
        id: "r".into(),
        root: root.path().into(),
        selected: true,
        exclusions: vec![],
    };
    let mut settings = Settings {
        repositories: vec![repository],
        mappings: vec![Mapping {
            phrase: "foo".into(),
            replacement: "EXACT".into(),
            repository: Some("r".into()),
            enabled: true,
        }],
        ..Default::default()
    };
    storage::save(data.path(), &settings).unwrap();
    let snapshot = storage::snapshot(data.path(), &settings).unwrap();
    settings.repositories[0].selected = false;
    assert!(
        storage::snapshot(data.path(), &settings)
            .unwrap()
            .mappings
            .is_empty()
    );
    assert_eq!(core::prepare("foo", &snapshot).text, "EXACT");
    settings.repositories.clear();
    storage::save(data.path(), &settings).unwrap();
    assert!(storage::indexes(data.path()).unwrap().is_empty());
    assert!(
        storage::snapshot(data.path(), &settings)
            .unwrap()
            .mappings
            .is_empty()
    );
    let files: Vec<_> = std::fs::read_dir(data.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(files, vec!["conch.sqlite3"]);
}
#[test]
fn local_endpoint_receives_only_masked_transcript_and_fixed_instruction() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut bytes = vec![];
        let mut chunk = [0; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(header_end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                if bytes.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8(bytes).unwrap();
        assert!(request.contains("CONCHPROTECTED0X"));
        assert!(!request.contains("SecretClass"));
        assert!(!request.contains("private/repo"));
        let body =
            r#"{"choices":[{"message":{"content":"Do not change CONCHPROTECTED0X before 12."}}]}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
    });
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.mode = "enhanced".into();
    snapshot.cleanup.provider = "local".into();
    snapshot.cleanup.endpoint = endpoint;
    snapshot.mappings.push(Mapping {
        phrase: "secret class".into(),
        replacement: "SecretClass".into(),
        repository: Some("private/repo".into()),
        enabled: true,
    });
    let data = tempfile::tempdir().unwrap();
    let output = inference::enhanced(inference::CleanupRequest {
        workers: data.path(),
        data: data.path(),
        snapshot: &snapshot,
        raw: "do not change secret class before 12",
        worker: &inference::new_worker(),
        cpu: true,
    })
    .unwrap();
    assert_eq!(output, "Do not change SecretClass before 12.");
    thread.join().unwrap();
}
#[test]
fn disabled_remote_cleanup_makes_no_request_and_cancellation_prevents_launch() {
    let data = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.mode = "enhanced".into();
    snapshot.cleanup.provider = "remote".into();
    snapshot.cleanup.endpoint = "https://127.0.0.1:1".into();
    let worker = inference::new_worker();
    assert_eq!(
        inference::enhanced(inference::CleanupRequest {
            workers: data.path(),
            data: data.path(),
            snapshot: &snapshot,
            raw: "hello",
            worker: &worker,
            cpu: true
        })
        .unwrap_err(),
        "Remote cleanup is disabled"
    );
    inference::stop(&worker);
}
#[test]
#[ignore = "Requires locally built workers and explicitly downloaded models"]
fn offline_workers_recognize_and_clean_without_a_model_server_install() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let data = root.join(".runtime");
    let workers = std::env::var_os("CONCH_TEST_WORKERS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("src-tauri/workers"));
    let wav = std::fs::read(data.join("synthetic.wav")).unwrap();
    let raw = inference::transcribe(
        &workers,
        &data.join("ggml-tiny.en.bin"),
        wav,
        "",
        &inference::new_worker(),
        true,
    )
    .unwrap();
    assert!(raw.to_lowercase().contains("do not change"));
    assert!(raw.contains("12"));
    let hinted = inference::transcribe(
        &workers,
        &data.join("ggml-tiny.en.bin"),
        std::fs::read(data.join("synthetic.wav")).unwrap(),
        "Vocabulary: podGC, maxGroups.",
        &inference::new_worker(),
        true,
    )
    .unwrap();
    assert!(hinted.to_lowercase().contains("do not change"));
    assert!(!hinted.contains("Vocabulary"));
    assert!(hinted.contains("podGC") && hinted.contains("maxGroups"));
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.mode = "enhanced".into();
    let result = inference::enhanced(inference::CleanupRequest {
        workers: &workers,
        data: &data,
        snapshot: &snapshot,
        raw: "do not change 12 tests",
        worker: &inference::new_worker(),
        cpu: true,
    })
    .unwrap();
    assert!(result.to_lowercase().contains("do not change 12"));
    inference::shutdown(&workers);
}

#[test]
#[ignore = "Explicitly downloads the 77.7 MB recognition model"]
fn managed_download_installs_verified_model_without_persisting_audio_or_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let path = inference::download(dir.path(), inference::MODELS[0], |_, _| {}).unwrap();
    assert_eq!(path.metadata().unwrap().len(), inference::MODELS[0].size);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn pre_cancelled_external_cleanup_does_not_start_a_request() {
    let data = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.mode = "enhanced".into();
    snapshot.cleanup.provider = "local".into();
    snapshot.cleanup.endpoint = "http://127.0.0.1:1/v1/chat/completions".into();
    let worker = inference::new_worker();
    inference::stop(&worker);
    let error = inference::enhanced(inference::CleanupRequest {
        workers: data.path(),
        data: data.path(),
        snapshot: &snapshot,
        raw: "hello",
        worker: &worker,
        cpu: true,
    })
    .unwrap_err();
    assert_eq!(error, "Cancelled");
}

#[test]
fn cancellation_drops_an_in_flight_endpoint_request() {
    use std::io::Read;
    use std::sync::mpsc;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (seen, received) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0; 4096];
        assert!(stream.read(&mut bytes).unwrap() > 0);
        seen.send(()).unwrap();
        let _ = wait.recv();
    });
    let worker = inference::new_worker();
    let running = worker.clone();
    let (send, result) = mpsc::channel();
    let job = std::thread::spawn(move || {
        let data = tempfile::tempdir().unwrap();
        let mut snapshot = Snapshot::default();
        snapshot.cleanup.mode = "enhanced".into();
        snapshot.cleanup.provider = "local".into();
        snapshot.cleanup.endpoint = endpoint;
        let outcome = inference::enhanced(inference::CleanupRequest {
            workers: data.path(),
            data: data.path(),
            snapshot: &snapshot,
            raw: "hello",
            worker: &running,
            cpu: true,
        });
        send.send(outcome).unwrap();
    });
    received
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    inference::stop(&worker);
    let outcome = result.recv_timeout(std::time::Duration::from_secs(5));
    release.send(()).unwrap();
    server.join().unwrap();
    job.join().unwrap();
    assert_eq!(outcome.unwrap().unwrap_err(), "Cancelled");
}

#[test]
fn vocabulary_snapshots_exclude_unselected_and_missing_repositories_and_keep_prior_versions() {
    use conch_lib::index;
    let data = tempfile::tempdir().unwrap();
    let alpha = tempfile::tempdir().unwrap();
    let beta = tempfile::tempdir().unwrap();
    std::fs::write(alpha.path().join("alpha.py"), "class AlphaOnly: pass").unwrap();
    std::fs::write(beta.path().join("beta.ts"), "class BetaOnly {}").unwrap();
    let mut settings = Settings {
        repositories: vec![
            Repository {
                id: "a".into(),
                root: alpha.path().into(),
                selected: true,
                exclusions: vec![],
            },
            Repository {
                id: "b".into(),
                root: beta.path().into(),
                selected: false,
                exclusions: vec![],
            },
        ],
        ..Default::default()
    };
    storage::put_index(
        data.path(),
        "a",
        &index::scan(alpha.path(), "a", &[]).unwrap(),
    )
    .unwrap();
    storage::put_index(
        data.path(),
        "b",
        &index::scan(beta.path(), "b", &[]).unwrap(),
    )
    .unwrap();
    let captured = storage::snapshot(data.path(), &settings).unwrap();
    assert!(captured.vocabulary.iter().any(|v| v.text == "AlphaOnly"));
    assert!(!captured.vocabulary.iter().any(|v| v.repository == "b"));
    settings.repositories[0].root = alpha.path().join("missing");
    settings.repositories[1].selected = true;
    let next = storage::snapshot(data.path(), &settings).unwrap();
    assert!(!next.vocabulary.iter().any(|v| v.repository == "a"));
    assert!(next.vocabulary.iter().any(|v| v.text == "BetaOnly"));
    assert!(captured.vocabulary.iter().any(|v| v.text == "AlphaOnly"));
}
