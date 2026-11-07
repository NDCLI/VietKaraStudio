//! Interactive UI prototype. No project I/O, playback, AI or render services.
use crate::{text_input::TextInput, theme};
use gpui::{
    App, Application, Bounds, Context, Entity, MouseButton, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Timing,
    Design,
    Export,
}

#[derive(Clone, Copy)]
enum Action {
    Mode(Mode),
    Select(usize),
    Media(bool),
    Dialog(&'static str),
    Close,
    ApplyText,
    Size(i32),
    Color,
    Zoom(i32),
    Seek(i32),
    Role,
    Reset,
}

struct Shell {
    mode: Mode,
    media: bool,
    lines: Vec<String>,
    selected: usize,
    input: Entity<TextInput>,
    font_size: i32,
    color: usize,
    zoom: i32,
    position: i32,
    female: bool,
    dialog: Option<&'static str>,
    status: String,
}

impl Shell {
    fn act(&mut self, action: Action, cx: &mut Context<Self>) {
        match action {
            Action::Mode(mode) => self.mode = mode,
            Action::Media(media) => self.media = media,
            Action::Select(index) => {
                self.selected = index;
                self.position = index as i32 * 5;
                let text = self.lines[index].clone();
                self.input.update(cx, |input, cx| {
                    input.set_value(&text);
                    cx.notify();
                });
            }
            Action::Dialog(title) => self.dialog = Some(title),
            Action::Close => self.dialog = None,
            Action::ApplyText => {
                let text = self.input.read(cx).value();
                if !text.trim().is_empty() {
                    self.lines[self.selected] = text;
                    self.status = "Đã đổi lời mẫu trong phiên UI".into();
                }
            }
            Action::Size(delta) => self.font_size = (self.font_size + delta).clamp(20, 64),
            Action::Color => self.color = (self.color + 1) % 3,
            Action::Zoom(delta) => self.zoom = (self.zoom + delta).clamp(60, 180),
            Action::Seek(delta) => self.position = (self.position + delta).clamp(0, 30),
            Action::Role => self.female = !self.female,
            Action::Reset => {
                self.position = 0;
                self.status = "Đã đưa con trỏ mẫu về đầu".into();
            }
        }
        cx.notify();
    }

    fn button(
        &self,
        label: impl Into<String>,
        action: Action,
        active: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(12.))
            .bg(rgb(if active {
                theme::ACCENT_SURFACE
            } else {
                theme::CANVAS
            }))
            .hover(|style| style.bg(rgb(theme::SURFACE_HOVER)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.act(action, cx)),
            )
            .child(label.into())
    }
}

fn caption(text: impl Into<String>) -> gpui::Div {
    div()
        .text_size(px(11.))
        .text_color(rgb(theme::TEXT_MUTED))
        .child(text.into())
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = [0xffd166, 0x72d6ce, 0xf39cc5][self.color];
        let left = div()
            .w(px(240.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_lg()
            .bg(rgb(theme::SURFACE))
            .overflow_hidden()
            .child(caption("PROJECT · BÀI HÁT MẪU"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.button("Lời bài hát", Action::Media(false), !self.media, cx))
                    .child(self.button("Media", Action::Media(true), self.media, cx)),
            )
            .child(if self.media {
                div()
                    .id("demo-media")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(caption("NGUỒN ÂM THANH"))
                    .child(self.button("＋ Beat gốc", Action::Dialog("Nhập beat"), false, cx))
                    .child(self.button(
                        "＋ Bản có vocal",
                        Action::Dialog("Nhập bản tham chiếu"),
                        false,
                        cx,
                    ))
                    .child(caption("HÌNH ẢNH & VIDEO"))
                    .child(self.button("＋ Nền ảnh / video", Action::Dialog("Chọn nền"), false, cx))
                    .child(self.button("＋ Logo", Action::Dialog("Chọn logo"), false, cx))
            } else {
                div()
                    .id("demo-lyrics")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.button(
                        "＋ Nhập SRT / TXT",
                        Action::Dialog("Nhập lời bài hát"),
                        false,
                        cx,
                    ))
                    .child(self.button(
                        "AI nhận lời",
                        Action::Dialog("Nhận dạng lời bằng AI"),
                        false,
                        cx,
                    ))
                    .children(self.lines.iter().enumerate().map(|(index, line)| {
                        self.button(
                            format!("{:02}  {}", index + 1, line),
                            Action::Select(index),
                            self.selected == index,
                            cx,
                        )
                    }))
            });
        let preview = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_lg()
            .bg(rgb(theme::SURFACE))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(caption("XEM TRƯỚC"))
                    .child(caption("16:9 · Dữ liệu mẫu")),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(160.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .rounded_lg()
                    .bg(rgb(0x101c2c))
                    .gap_4()
                    .p_4()
                    .child(caption("VIETKARA STUDIO"))
                    .child(
                        div()
                            .text_center()
                            .text_size(px(self.font_size as f32))
                            .text_color(rgb(color))
                            .child(self.lines[self.selected].clone()),
                    )
                    .child(caption("Chọn câu hoặc chỉnh chữ để xem thay đổi ngay")),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(self.button("−5s", Action::Seek(-5), false, cx))
                    .child(self.button("▶ Phát", Action::Dialog("Phát nhạc"), true, cx))
                    .child(self.button("■ Về đầu", Action::Reset, false, cx))
                    .child(self.button("+5s", Action::Seek(5), false, cx))
                    .child(caption(format!(
                        "00:{:02} / 00:30 · con trỏ mẫu",
                        self.position
                    ))),
            );
        let mut inspector = div()
            .id("demo-inspector")
            .w(px(290.))
            .flex_shrink_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_lg()
            .bg(rgb(theme::SURFACE));
        inspector = match self.mode {
            Mode::Timing => inspector
                .child(caption("LỜI & THỜI GIAN"))
                .child(caption(format!("Câu đang chọn: {}", self.selected + 1)))
                .child(self.input.clone())
                .child(self.button("Áp dụng lời mẫu", Action::ApplyText, true, cx))
                .child(caption(format!(
                    "Bắt đầu 00:{:02} · Kết thúc 00:{:02}",
                    self.selected * 5,
                    self.selected * 5 + 4
                )))
                .child(self.button(
                    if self.female {
                        "Vai mẫu: Nữ"
                    } else {
                        "Vai mẫu: Nam"
                    },
                    Action::Role,
                    false,
                    cx,
                ))
                .child(self.button(
                    "Căn bằng Space",
                    Action::Dialog("Căn lời thủ công"),
                    false,
                    cx,
                ))
                .child(self.button(
                    "AI căn câu đang chọn",
                    Action::Dialog("AI căn câu"),
                    false,
                    cx,
                )),
            Mode::Design => inspector
                .child(caption("THIẾT KẾ VIDEO"))
                .child(caption("CHỮ KARAOKE"))
                .child(self.button("Chọn font", Action::Dialog("Thư viện font"), false, cx))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(self.button("−", Action::Size(-2), false, cx))
                        .child(caption(format!("Cỡ mẫu: {}", self.font_size)))
                        .child(self.button("+", Action::Size(2), false, cx)),
                )
                .child(self.button("Đổi màu chữ mẫu", Action::Color, true, cx))
                .child(self.button(
                    "Preset kiểu chữ",
                    Action::Dialog("Thư viện preset"),
                    false,
                    cx,
                ))
                .child(self.button(
                    "Viền / bóng / hiệu ứng",
                    Action::Dialog("Hiệu ứng chữ"),
                    false,
                    cx,
                ))
                .child(caption("BỐ CỤC & TRANG TRÍ"))
                .child(self.button("Nền & scene", Action::Dialog("Nền và scene"), false, cx))
                .child(self.button(
                    "Tiêu đề & logo",
                    Action::Dialog("Tiêu đề và logo"),
                    false,
                    cx,
                ))
                .child(self.button(
                    "Song ca & countdown",
                    Action::Dialog("Song ca và đếm ngược"),
                    false,
                    cx,
                )),
            Mode::Export => inspector
                .child(caption("XUẤT VIDEO"))
                .child(caption("Thiết lập mẫu · chưa tạo file"))
                .child(self.button("MP4 · H.264", Action::Dialog("Định dạng video"), false, cx))
                .child(self.button(
                    "1920 × 1080 · 30 FPS",
                    Action::Dialog("Độ phân giải và FPS"),
                    false,
                    cx,
                ))
                .child(self.button(
                    "Âm thanh: beat gốc",
                    Action::Dialog("Âm thanh xuất"),
                    false,
                    cx,
                ))
                .child(self.button(
                    "Chọn thư mục lưu",
                    Action::Dialog("Thư mục xuất"),
                    false,
                    cx,
                ))
                .child(self.button("Xuất video", Action::Dialog("Xuất video"), true, cx))
                .child(self.button(
                    "Hàng đợi batch",
                    Action::Dialog("Hàng đợi xuất video"),
                    false,
                    cx,
                )),
        };
        let timeline = div().flex_shrink_0().p_3().rounded_lg().bg(rgb(theme::SURFACE))
            .child(div().flex().flex_wrap().items_center().gap_2().mb_3()
                .child(caption("TIMELINE · DỮ LIỆU MẪU"))
                .child(self.button("Zoom −", Action::Zoom(-20), false, cx))
                .child(caption(format!("{}%", self.zoom)))
                .child(self.button("Zoom +", Action::Zoom(20), false, cx))
                .child(self.button("Mốc liên kết", Action::Dialog("Chế độ chỉnh mốc"), false, cx)))
            .child(div().id("demo-timeline").overflow_x_scroll()
                .child(div().w(px(8. * self.zoom as f32)).flex().flex_col().gap_2()
                    .child(caption("00:00          00:05          00:10          00:15          00:20          00:25          00:30"))
                    .child(div().flex().gap_1().items_center().h(px(30.))
                        .children((0..100).map(|i| div().flex_1().h(px(5. + ((i * 17 + 7) % 23) as f32)).bg(rgb(0x528a99)))))
                    .children((0..2).map(|lane| div().flex().gap_2().h(px(34.))
                        .child(caption(if lane == 0 {"Nam"} else {"Nữ"}))
                        .children(self.lines.iter().enumerate().filter(|(i, _)| i % 2 == lane).map(|(i, line)|
                            self.button(line.clone(), Action::Select(i), self.selected == i, cx)))))));
        div().size_full().relative().bg(rgb(theme::CANVAS)).text_color(rgb(theme::TEXT)).text_size(px(13.))
            .child(div().id("demo-workspace").size_full().overflow_y_scroll().overflow_x_scroll()
                .child(div().flex().flex_col().min_w(px(1000.)).min_h(px(760.)).h_full().p_3().gap_3()
                    .child(div().flex().flex_wrap().items_center().gap_3()
                        .child(div().text_size(px(19.)).child("VietKara Studio"))
                        .child(caption("UI DEMO · CHƯA NỐI ENGINE"))
                        .child(self.button("Project mới", Action::Dialog("Tạo project"), false, cx))
                        .child(self.button("Mở", Action::Dialog("Mở project"), false, cx))
                        .child(self.button("Lưu", Action::Dialog("Lưu project"), false, cx))
                        .child(self.button("Xuất video", Action::Mode(Mode::Export), true, cx)))
                    .child(div().flex().gap_2()
                        .child(self.button("01  Căn lời", Action::Mode(Mode::Timing), self.mode == Mode::Timing, cx))
                        .child(self.button("02  Thiết kế", Action::Mode(Mode::Design), self.mode == Mode::Design, cx))
                        .child(self.button("03  Xuất video", Action::Mode(Mode::Export), self.mode == Mode::Export, cx)))
                    .child(div().flex().flex_1().min_h(px(300.)).gap_3().child(left).child(preview).child(inspector))
                    .child(timeline)
                    .child(caption(self.status.clone()))))
            .when_some(self.dialog, |root, title| root.child(
                div().absolute().top_0().left_0().size_full().bg(gpui::rgba(0x000000aa)).flex().items_center().justify_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().w(px(480.)).p_6().rounded_lg().bg(rgb(theme::SURFACE)).flex().flex_col().gap_4()
                        .child(div().text_size(px(20.)).child(title))
                        .child("Khung chức năng đã có. Phần xử lý sẽ được nối ở giai đoạn sau.")
                        .child(caption("Bản UI mẫu không đọc/ghi project, chạy AI, phát audio hoặc tạo video."))
                        .child(self.button("Đóng", Action::Close, true, cx)))))
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        cx.bind_keys(crate::text_input::key_bindings());
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                let lines = vec![
                    "Một ngày nắng lên".to_owned(),
                    "Ta cùng hát vang".to_owned(),
                    "Qua từng con phố".to_owned(),
                    "Gửi lời yêu thương".to_owned(),
                    "Bầu trời trong xanh".to_owned(),
                    "Giữ mãi câu ca".to_owned(),
                ];
                let input = cx.new(|cx| TextInput::new(cx, &lines[0]));
                cx.new(|_| Shell {
                    mode: Mode::Timing,
                    media: false,
                    lines,
                    selected: 0,
                    input,
                    font_size: 36,
                    color: 0,
                    zoom: 100,
                    position: 0,
                    female: false,
                    dialog: None,
                    status: "UI mẫu · Thay đổi chỉ tồn tại trong phiên này".into(),
                })
            },
        )
        .expect("Không mở được UI mẫu VietKara");
        cx.activate(true);
    });
}
