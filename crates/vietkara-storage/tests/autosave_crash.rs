use std::{
    fs::{File, FileTimes},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime},
};
use vietkara_domain::Project;
use vietkara_storage::{
    autosave::{recovery_candidate, save_autosave},
    load_project, save_project,
};

#[test]
fn autosave_writer_child() {
    let Some(main) = std::env::var_os("VIETKARA_CRASH_TEST_MAIN") else {
        return;
    };
    let main = std::path::PathBuf::from(main);
    let mut project = load_project(&main).unwrap();
    project.name = "Đã chỉnh trước crash".into();
    project
        .settings
        .extra
        .insert("future_recovery".into(), serde_json::json!({"keep":true}));
    save_autosave(&project, &main).unwrap();
    std::fs::write(main.with_extension("ready"), b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn killed_writer_leaves_recoverable_snapshot_and_unchanged_main() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("Bài hát.json");
    let project = Project::new("Bản chính");
    save_project(&project, &main).unwrap();
    File::options()
        .write(true)
        .open(&main)
        .unwrap()
        .set_times(FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(10)))
        .unwrap();
    let original = std::fs::read(&main).unwrap();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "autosave_writer_child", "--nocapture"])
            .env("VIETKARA_CRASH_TEST_MAIN", &main)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !main.with_extension("ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "writer terminated before snapshot"
        );
        assert!(
            Instant::now() < deadline,
            "writer did not publish snapshot in time"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    assert!(!status.success());
    let recovered = recovery_candidate(&main).unwrap().unwrap().accept();
    assert_eq!(recovered.id, project.id);
    assert_eq!(recovered.name, "Đã chỉnh trước crash");
    assert_eq!(
        recovered.settings.extra["future_recovery"],
        serde_json::json!({"keep":true})
    );
    assert_eq!(std::fs::read(&main).unwrap(), original);
    assert_eq!(load_project(&main).unwrap().name, "Bản chính");
}
