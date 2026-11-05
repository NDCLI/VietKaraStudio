use gpui::{
    App, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb,
    size,
};
use gpui::{Application, ApplicationExt};
use vietkara_domain::{Project, Word};

struct Studio {
    project: Project,
    playing: bool,
}

impl Render for Studio {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let transport = if self.playing {
            "Ⅱ  Tạm dừng"
        } else {
            "▶  Phát"
        };
        let lyric_rows = self.project.words.iter().map(|line: &Word| {
            div()
                .flex()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(rgb(0x292c34))
                .child(format!("{:05.2}  {}", line.start, line.text))
                .child(format!("{:05.2}", line.end))
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x17191f))
            .text_color(rgb(0xe8e9ed))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(0x20232b))
                    .child(format!("VietKara Studio  /  {}", self.project.name))
                    .child("Mở project    Lưu    Xuất video"),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .p_3()
                    .child(
                        div()
                            .w(px(220.))
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child("PROJECT")
                            .child(div().mt_3().child(if self.project.beat.is_some() {
                                "Beat gốc • đã nạp"
                            } else {
                                "Beat gốc"
                            }))
                            .child(div().mt_2().child(if self.project.reference.is_some() {
                                "Bản tham chiếu • đã nạp"
                            } else {
                                "Bản tham chiếu"
                            }))
                            .child(div().mt_2().child("Video nền"))
                            .child(div().mt_6().child("Preset chữ"))
                            .child(div().mt_2().child("Vai & màu song ca")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .bg(rgb(0x20232b))
                                    .rounded_md()
                                    .p_4()
                                    .child("XEM TRƯỚC")
                                    .child(
                                        div()
                                            .flex()
                                            .flex_1()
                                            .justify_center()
                                            .items_center()
                                            .child("Nạp beat để xem trước video karaoke"),
                                    ),
                            )
                            .child(
                                div()
                                    .h(px(220.))
                                    .bg(rgb(0x20232b))
                                    .rounded_md()
                                    .p_3()
                                    .child("TIMELINE  •  waveform và căn lời")
                                    .children(lyric_rows),
                            ),
                    )
                    .child(
                        div()
                            .w(px(250.))
                            .bg(rgb(0x20232b))
                            .rounded_md()
                            .p_3()
                            .child("CÂU ĐANG CHỌN")
                            .child(div().mt_3().child("Lời bài hát"))
                            .child(
                                div()
                                    .mt_2()
                                    .p_2()
                                    .bg(rgb(0x17191f))
                                    .child("Chào mừng đến VietKara"),
                            )
                            .child(div().mt_4().child("Bắt đầu       00:00.00"))
                            .child(div().mt_2().child("Kết thúc       00:00.00"))
                            .child(div().mt_6().child("Space để căn lời theo nhạc")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_5()
                    .py_3()
                    .bg(rgb(0x20232b))
                    .child(format!(
                        "{}     ⏮     ⏭      00:00.00 / 00:00.00",
                        transport
                    ))
                    .child("Sẵn sàng"),
            )
    }
}

fn main() {
    let project = Project::new("Bài hát mới");
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Studio {
                    project,
                    playing: false,
                })
            },
        )
        .expect("Không mở được cửa sổ VietKara Studio");
        cx.activate(true);
    });
}
