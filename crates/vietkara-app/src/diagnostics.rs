use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
enum State {
    Ok,
    Warning,
    Error,
}

struct Check {
    name: &'static str,
    state: State,
    detail: String,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            state: State::Ok,
            detail: detail.into(),
        }
    }
    fn warning(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            state: State::Warning,
            detail: detail.into(),
        }
    }
    fn error(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            state: State::Error,
            detail: detail.into(),
        }
    }
}

pub(super) fn collect() -> String {
    let mut checks = Vec::new();
    let worker = find_worker_script();
    match &worker {
        Ok(_) => checks.push(Check::ok("AI worker", "Đã tìm thấy ai_worker.py")),
        Err(_) => checks.push(Check::warning(
            "AI worker",
            "Không tìm thấy ai_worker.py; đặt VIETKARA_AI_WORKER trỏ tới worker đã cài.",
        )),
    }

    let python = python_command();
    match run_bounded(&python, &["--version"], Duration::from_secs(5)) {
        Ok(output) if output.status.success() => {
            let version = first_line(&[output.stdout, output.stderr].concat());
            checks.push(Check::ok("Python", version));
            let module_script = "import importlib.util,json; names=('torch','stable_whisper','demucs','numpy','soundfile'); print(json.dumps({name: importlib.util.find_spec(name) is not None for name in names}))";
            match run_bounded(&python, &["-c", module_script], Duration::from_secs(8)) {
                Ok(output) if output.status.success() => {
                    let modules = serde_json::from_slice::<Value>(&output.stdout).ok();
                    let missing = modules.as_ref().and_then(Value::as_object).map(|values| {
                        values
                            .iter()
                            .filter(|(_, present)| !present.as_bool().unwrap_or(false))
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>()
                    });
                    match missing {
                        Some(names) if names.is_empty() => checks.push(Check::ok(
                            "Thư viện AI",
                            "Đã có torch, stable-whisper, demucs, numpy và soundfile",
                        )),
                        Some(names) => checks.push(Check::warning(
                            "Thư viện AI",
                            format!("Thiếu module: {}", names.join(", ")),
                        )),
                        None => checks.push(Check::warning(
                            "Thư viện AI",
                            "Python chạy được nhưng không đọc được kết quả kiểm tra module.",
                        )),
                    }
                }
                Ok(_) => checks.push(Check::warning(
                    "Thư viện AI",
                    "Python chạy được nhưng chưa kiểm tra được module AI.",
                )),
                Err(_) => checks.push(Check::warning(
                    "Thư viện AI",
                    "Kiểm tra module AI quá thời gian hoặc không khởi chạy được.",
                )),
            }
        }
        Ok(_) => checks.push(Check::warning(
            "Python",
            "Python không chạy được; chức năng AI cần runtime Python 3.12.",
        )),
        Err(_) => checks.push(Check::warning(
            "Python",
            "Không tìm thấy runtime Python; đặt VIETKARA_PYTHON trỏ tới python.exe.",
        )),
    }

    check_binary("FFmpeg", "ffmpeg", true, &mut checks);
    check_binary("FFprobe", "ffprobe", false, &mut checks);

    let (font_families, font_files) = crate::fonts::inventory_counts();
    if font_families > 0 && font_files > 0 {
        checks.push(Check::ok(
            "Font",
            format!("{font_families} họ, {font_files} file font nhúng"),
        ));
    } else {
        checks.push(Check::error("Font", "Không đọc được thư viện font nhúng."));
    }

    let data_root = data_root();
    match check_data_writable(&data_root) {
        Ok(()) => checks.push(Check::ok(
            "Dữ liệu ứng dụng",
            "Thư mục dữ liệu ghi được; báo cáo được giữ cục bộ.",
        )),
        Err(_) => checks.push(Check::warning(
            "Dữ liệu ứng dụng",
            "Không ghi được thư mục dữ liệu; báo cáo chỉ còn trong cửa sổ này.",
        )),
    }

    let mut report = format_report(&checks);
    if check_data_writable(&data_root).is_ok() && save_report(&data_root, &report).is_err() {
        checks.push(Check::warning(
            "Lưu báo cáo",
            "Không ghi được diagnostics/latest.txt vào dữ liệu ứng dụng.",
        ));
        report = format_report(&checks);
    }
    report
}

fn check_binary(name: &'static str, executable: &str, required: bool, checks: &mut Vec<Check>) {
    match vietkara_media::find_binary(executable) {
        Ok(path) => match run_bounded(path.as_os_str(), &["-version"], Duration::from_secs(5)) {
            Ok(output) if output.status.success() => checks.push(Check::ok(
                name,
                first_line(&[output.stdout, output.stderr].concat()),
            )),
            _ if required => checks.push(Check::error(
                name,
                "Đã tìm thấy file nhưng không chạy được; kiểm tra quyền hoặc bản cài.",
            )),
            _ => checks.push(Check::warning(
                name,
                "Không chạy được FFprobe; app sẽ dùng FFmpeg làm phương án dự phòng.",
            )),
        },
        Err(_) if required => checks.push(Check::error(
            name,
            "Không tìm thấy; đặt ffmpeg.exe trong bin cạnh app hoặc thêm vào PATH.",
        )),
        Err(_) => checks.push(Check::warning(
            name,
            "Không tìm thấy; app có thể dùng FFmpeg để đọc thông tin media.",
        )),
    }
}

fn run_bounded(
    executable: &std::ffi::OsStr,
    arguments: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let mut child = Command::new(executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "spawn failed".to_owned())?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|_| "wait failed".to_owned())? {
            Some(_) => {
                return child
                    .wait_with_output()
                    .map_err(|_| "read failed".to_owned());
            }
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn python_command() -> std::ffi::OsString {
    std::env::var_os("VIETKARA_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into())
}

fn find_worker_script() -> Result<PathBuf, ()> {
    if let Some(path) = std::env::var_os("VIETKARA_AI_WORKER").map(PathBuf::from) {
        return path.is_file().then_some(path).ok_or(());
    }
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.extend(parent.ancestors().map(PathBuf::from));
    }
    if let Ok(directory) = std::env::current_dir() {
        roots.extend(directory.ancestors().map(PathBuf::from));
    }
    roots.dedup();
    roots
        .into_iter()
        .map(|root| root.join("ai_worker.py"))
        .find(|path| path.is_file())
        .ok_or(())
}

fn data_root() -> PathBuf {
    if let Some(path) = std::env::var_os("OBS_DATA_DIR") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(path).join("VietKaraStudio")
    } else {
        std::env::current_dir().unwrap_or_default().join("data")
    }
}

fn check_data_writable(root: &Path) -> Result<(), String> {
    let directory = root.join("diagnostics");
    std::fs::create_dir_all(&directory).map_err(|_| "create failed".to_owned())?;
    let staged =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "write failed".to_owned())?;
    drop(staged);
    Ok(())
}

fn save_report(root: &Path, report: &str) -> Result<(), String> {
    use std::io::Write;
    let directory = root.join("diagnostics");
    std::fs::create_dir_all(&directory).map_err(|_| "create failed".to_owned())?;
    let mut staged =
        tempfile::NamedTempFile::new_in(&directory).map_err(|_| "stage failed".to_owned())?;
    staged
        .write_all(report.as_bytes())
        .map_err(|_| "write failed".to_owned())?;
    staged
        .as_file()
        .sync_all()
        .map_err(|_| "sync failed".to_owned())?;
    staged
        .persist(directory.join("latest.txt"))
        .map_err(|_| "publish failed".to_owned())?;
    Ok(())
}

fn first_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Đã chạy thành công")
        .chars()
        .filter(|character| !character.is_control())
        .take(140)
        .collect()
}

fn format_report(checks: &[Check]) -> String {
    let mut report = format!(
        "VietKara Studio · Chẩn đoán cục bộ · {}\nKhông gửi dữ liệu ra mạng; không đọc project hoặc media.\n\n",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    for check in checks {
        let marker = match check.state {
            State::Ok => "OK",
            State::Warning => "CẢNH BÁO",
            State::Error => "LỖI",
        };
        report.push_str(&format!("[{marker}] {}: {}\n", check.name, check.detail));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_local_and_excludes_machine_paths() {
        let report = format_report(&[
            Check::ok("Python", "Python 3.12.1"),
            Check::warning("FFprobe", "Không tìm thấy"),
        ]);
        assert!(report.contains("Chẩn đoán cục bộ"));
        assert!(report.contains("Không gửi dữ liệu ra mạng"));
        assert!(!report.contains("C:\\Users\\"));
        assert!(!report.contains("G:\\Project\\"));
    }

    #[test]
    fn output_line_removes_terminal_control_characters_and_is_bounded() {
        assert_eq!(first_line(b"\x1b[31mFFmpeg 8.0\nsecond"), "[31mFFmpeg 8.0");
        assert_eq!(first_line(&[b'a'; 200]).len(), 140);
    }
}
