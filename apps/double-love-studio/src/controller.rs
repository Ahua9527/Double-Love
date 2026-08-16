use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{Bounds, Context, Pixels, Subscription, Window, point, px, size};
use gpui_component::Theme;

use crate::fixtures::{FixtureSet, aozora_diary};

/// 合成时间线总时长（秒），seek 换算与绘制共用。
pub const TIMELINE_TOTAL_SEC: f32 = 120.;
/// 时间线绘制区相对容器的左留白（px，轨道标签宽度），seek 换算与绘制共用。
pub const TIMELINE_LEFT_INSET: f32 = 28.;
/// 时间线绘制区相对容器的右留白（px），与左留白合计 40px。
pub const TIMELINE_RIGHT_INSET: f32 = 12.;

/// StudioController 是 GPUI View 与 Engine 之间的唯一连接层。
/// View 不直接访问 SQLite、Shell 或任意路径；Finder 拖入只授权读取对应 XML/CSV。
pub struct StudioController {
    fixtures: FixtureSet,
    selected_clip: usize,
    notice: Option<String>,
    /// 播放头位置（0.0–1.0，对应 TIMELINE_TOTAL_SEC）。
    playhead: f32,
    /// 左键按住拖动播放头中。
    scrubbing: bool,
    /// 时间线容器的最新布局 bounds（canvas prepaint 每帧写入），供鼠标事件换算坐标。
    timeline_bounds: Rc<RefCell<Bounds<Pixels>>>,
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
            playhead: 0.35,
            scrubbing: false,
            timeline_bounds: Rc::new(RefCell::new(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(0.), px(0.)),
            })),
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

    pub fn playhead(&self) -> f32 {
        self.playhead
    }

    /// 时间线 bounds 的共享句柄，render 时交给 canvas prepaint 每帧回写。
    pub fn timeline_bounds_handle(&self) -> Rc<RefCell<Bounds<Pixels>>> {
        self.timeline_bounds.clone()
    }

    /// 播放头时钟文本，如 `00:42 / 02:00`。
    pub fn playhead_clock(&self) -> String {
        let total = TIMELINE_TOTAL_SEC as u32;
        let seconds = (self.playhead * TIMELINE_TOTAL_SEC) as u32;
        format!(
            "{:02}:{:02} / {:02}:{:02}",
            seconds / 60,
            seconds % 60,
            total / 60,
            total % 60
        )
    }

    /// 把窗口 x 坐标换算为播放头位置（0.0–1.0），留白与绘制侧保持一致。
    pub fn seek_to_window_x(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let (offset, width) = {
            let bounds = self.timeline_bounds.borrow();
            let width: f32 =
                (bounds.size.width - px(TIMELINE_LEFT_INSET + TIMELINE_RIGHT_INSET)).into();
            let offset: f32 = (x - bounds.left() - px(TIMELINE_LEFT_INSET)).into();
            (offset, width)
        };
        if width <= 0. {
            return;
        }
        self.playhead = (offset / width).clamp(0., 1.);
        cx.notify();
    }

    /// 时间线上按下左键：开始拖动并立即定位。
    pub fn begin_scrub(&mut self, x: Pixels, cx: &mut Context<Self>) {
        self.scrubbing = true;
        self.seek_to_window_x(x, cx);
    }

    /// 拖动中（仅在 begin_scrub 之后生效；由调用方保证左键仍按住）。
    pub fn scrub_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if self.scrubbing {
            self.seek_to_window_x(x, cx);
        }
    }

    pub fn end_scrub(&mut self) {
        self.scrubbing = false;
    }

    pub fn select_clip(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_clip = index.min(self.fixtures.clips.len().saturating_sub(1));
        // 原型联动：选中行把播放头跳到该片段的示意位置；接真实数据时移除。
        self.playhead = (self.selected_clip as f32 + 0.5) / self.fixtures.clips.len() as f32;
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
