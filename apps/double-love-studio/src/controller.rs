use std::path::PathBuf;

use gpui::{Context, Subscription, Window};
use gpui_component::Theme;

use crate::fixtures::{FixtureSet, aozora_diary};

/// StudioController 是 GPUI View 与 Engine 之间的唯一连接层。
/// View 不直接访问 SQLite、Shell 或任意路径；Finder 拖入只授权读取对应 XML/CSV。
pub struct StudioController {
    fixtures: FixtureSet,
    selected_clip: usize,
    notice: Option<String>,
    _appearance: Subscription,
}

impl StudioController {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
            cx.notify();
        });
        Self {
            fixtures: aozora_diary(),
            selected_clip: 0,
            notice: None,
            _appearance: appearance,
        }
    }

    pub fn fixtures(&self) -> &FixtureSet {
        &self.fixtures
    }

    pub fn selected_index(&self) -> usize {
        self.selected_clip
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn select_clip(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_clip = index.min(self.fixtures.clips.len().saturating_sub(1));
        cx.notify();
    }

    pub fn set_notice(&mut self, notice: impl Into<String>, cx: &mut Context<Self>) {
        self.notice = Some(notice.into());
        cx.notify();
    }

    /// 接收 Finder 拖入：只记录路径数量并提示，不读取文件内容（真实解析属后续迭代）。
    pub fn receive_dropped_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let accepted = paths
            .iter()
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("xml") || ext.eq_ignore_ascii_case("csv")
                    })
            })
            .count();
        self.notice = Some(if accepted == 0 {
            "已接收拖入：未发现 XML/CSV 文件（本次仅记录路径，不解析内容）".to_string()
        } else {
            format!("已接收 {accepted} 个 XML/CSV 路径（只读授权，本迭代不解析内容）")
        });
        cx.notify();
    }
}
