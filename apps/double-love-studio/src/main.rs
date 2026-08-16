use gpui::*;
use gpui_component::*;

mod controller;
mod fixtures;
mod workspace;

use controller::StudioController;

fn main() {
    let app = Application::new();
    app.run(move |cx| {
        gpui_component::init(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1440.), px(900.)),
                cx,
            ))),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitleBar::title_bar_options()),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let view = cx.new(|cx| StudioController::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("failed to open main window");
    });
}
