use gpui::{prelude::*, *};
use vietkara_domain::Project;

struct DialogSpike {
    status: String,
    pending: bool,
    project: Option<Project>,
}

impl DialogSpike {
    fn drop_project(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let [path] = paths.paths() else {
            self.status = "Chỉ thả một file project mỗi lần".into();
            cx.notify();
            return;
        };
        let path = path.clone();
        self.pending = true;
        self.status = format!("Đang đọc file thả: {}", path.display());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { Project::load(path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(project) => {
                        this.status = format!(
                            "Đã đọc file thả: {} | {} từ",
                            project.name,
                            project.words.len()
                        );
                        this.project = Some(project);
                    }
                    Err(error) => this.status = format!("Không đọc được file thả: {error}"),
                }
                this.pending = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        self.status = "Đang mở dialog…".into();
        cx.notify();
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Chọn fixture project để kiểm tra đường dẫn Unicode".into()),
        });
        cx.spawn(async move |this, cx| {
            let mut loaded = None;
            let status = match result.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let display = path.display().to_string();
                        match cx
                            .background_executor()
                            .spawn(async move { Project::load(path) })
                            .await
                        {
                            Ok(project) => {
                                let status = format!(
                                    "Đã đọc: {} | {} từ | {}",
                                    project.name,
                                    project.words.len(),
                                    display
                                );
                                loaded = Some(project);
                                status
                            }
                            Err(error) => format!("Không đọc được project: {error}"),
                        }
                    } else {
                        "Dialog trả về danh sách rỗng".into()
                    }
                }
                Ok(Ok(None)) => "Đã hủy mở file".into(),
                Ok(Err(error)) => format!("Dialog lỗi: {error}"),
                Err(error) => format!("Kênh dialog đóng: {error}"),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(project) = loaded {
                    this.project = Some(project);
                }
                this.status = status;
                this.pending = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(project) = self.project.clone() else {
            self.status = "Hãy mở fixture project trước khi lưu bản sao".into();
            cx.notify();
            return;
        };
        let directory = match std::env::current_dir() {
            Ok(directory) => directory,
            Err(error) => {
                self.status = format!("Không lấy được thư mục hiện tại: {error}");
                cx.notify();
                return;
            }
        };
        self.pending = true;
        self.status = "Đang mở dialog lưu…".into();
        cx.notify();
        let result = cx.prompt_for_new_path(&directory, Some("Bài hát tiếng Việt.json"));
        cx.spawn(async move |this, cx| {
            let status = match result.await {
                Ok(Ok(Some(path))) => {
                    let display = path.display().to_string();
                    let result = cx
                        .background_executor()
                        .spawn(async move { save_and_verify(&project, &path) })
                        .await;
                    match result {
                        Ok(reopened) => format!(
                            "Đã lưu và đọc lại: {} | {} từ | {}",
                            reopened.name,
                            reopened.words.len(),
                            display
                        ),
                        Err(error) => format!("Lưu/đọc lại thất bại: {error}"),
                    }
                }
                Ok(Ok(None)) => "Đã hủy chọn đường dẫn lưu".into(),
                Ok(Err(error)) => format!("Dialog lỗi: {error}"),
                Err(error) => format!("Kênh dialog đóng: {error}"),
            };
            let _ = this.update(cx, |this, cx| {
                this.status = status;
                this.pending = false;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for DialogSpike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .on_drop(cx.listener(Self::drop_project))
            .size_full()
            .p_4()
            .bg(rgb(0xeeeeee))
            .text_color(rgb(0x17191f))
            .child("GPUI Windows: dialog native / thả một file project JSON")
            .child(
                div()
                    .mt_4()
                    .p_3()
                    .bg(rgb(0xffffff))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::open))
                    .child("Mở dialog chọn file"),
            )
            .child(
                div()
                    .mt_4()
                    .p_3()
                    .bg(rgb(0xffffff))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::save))
                    .child("Lưu bản sao project"),
            )
            .child(div().mt_4().child(self.status.clone()))
            .child(div().mt_4().child(format!(
                "Viewport: {:?}; scale: {}",
                window.viewport_size(),
                window.scale_factor()
            )))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(500.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| DialogSpike {
                    status: "Mở fixture rồi lưu bản sao để thử round-trip.".into(),
                    project: None,
                    pending: false,
                })
            },
        )
        .expect("Không mở được cửa sổ spike dialog");
        cx.activate(true);
    });
}

// Compare the entire serialized domain model, including flattened unknown fields.
fn save_and_verify(project: &Project, path: &std::path::Path) -> Result<Project, String> {
    let expected = serde_json::to_value(project).map_err(|e| e.to_string())?;
    project.save(path).map_err(|e| e.to_string())?;
    let reopened = Project::load(path).map_err(|e| e.to_string())?;
    let actual = serde_json::to_value(&reopened).map_err(|e| e.to_string())?;
    if expected != actual {
        return Err("Project đọc lại khác dữ liệu trước khi lưu".into());
    }
    Ok(reopened)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn unicode_round_trip_preserves_all_domain_fields() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vietkara-domain/tests/fixtures/project-v2.json");
        let project = Project::load(fixture).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("Bài hát tiếng Việt");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("Bản sao Điệp khúc.json");
        let reopened = save_and_verify(&project, &path).unwrap();
        assert_eq!(reopened.id, "0123456789abcdef0123456789abcdef");
        assert_eq!(reopened.extra["future_python_field"]["keep"], true);
        assert_eq!(
            serde_json::to_value(&project).unwrap(),
            serde_json::to_value(&reopened).unwrap()
        );
    }

    #[::core::prelude::v1::test]
    fn save_failure_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing-parent").join("copy.json");
        assert!(save_and_verify(&Project::new("test"), &path).is_err());
    }
}
