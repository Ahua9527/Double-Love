//! 工作区布局：按参照截图方向实现 —— 预览窗为视觉中心、底部对位时间线、
//! 右侧卡片列表面板、左侧极简边栏，落在 Navigator 三栏结构上。

use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{button::*, *};

use crate::controller::StudioController;
use crate::fixtures::{ClipStatus, FixtureClip, Rating};

/// 品牌粉：仅品牌标识与关键操作。
fn brand_pink() -> Hsla {
    rgb(0xEA2AA0).into()
}

/// 选中态蓝。
fn select_blue() -> Hsla {
    rgb(0x3366FF).into()
}

/// 播放头红。
fn playhead_red() -> Hsla {
    rgb(0xE5484D).into()
}

/// B 机位轨道色。
fn track_teal() -> Hsla {
    rgb(0x12A594).into()
}

/// 音频轨道色。
fn audio_green() -> Hsla {
    rgb(0x30A46C).into()
}

fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

fn status_color(status: ClipStatus, cx: &App) -> Hsla {
    let theme = cx.theme();
    match status {
        ClipStatus::Processed => theme.success,
        ClipStatus::Ignored => theme.muted_foreground,
        ClipStatus::Skipped => theme.warning,
        ClipStatus::Failed => theme.danger,
    }
}

fn status_label(status: ClipStatus) -> &'static str {
    match status {
        ClipStatus::Processed => "已处理",
        ClipStatus::Ignored => "已忽略",
        ClipStatus::Skipped => "已跳过",
        ClipStatus::Failed => "失败",
    }
}

impl Render for StudioController {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let foreground = theme.foreground;
        let muted_foreground = theme.muted_foreground;

        div()
            .id("workspace-root")
            .size_full()
            .v_flex()
            .bg(background)
            .text_color(foreground)
            .child(self.render_titlebar(cx))
            .when_some(self.notice().map(str::to_string), |this, notice| {
                this.child(render_notice(&notice, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .h_flex()
                    .child(self.render_sidebar(cx))
                    .child(self.render_center(cx))
                    .child(self.render_inspector(cx)),
            )
            .child(self.render_timeline(cx))
            .child(
                h_flex()
                    .h_7()
                    .flex_none()
                    .px_3()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(muted_foreground)
                    .child(self.render_health(cx))
                    .child(format!(
                        "{} ｜ CSV 20/21 ｜ rev 6",
                        self.fixtures().episode_label
                    )),
            )
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.receive_dropped_paths(paths.paths(), cx);
            }))
    }
}

impl StudioController {
    fn render_titlebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted_foreground = theme.muted_foreground;
        let border = theme.border;
        let fixtures = self.fixtures();
        let export_blocked = fixtures.export_blocked;

        TitleBar::new().child(
            h_flex()
                .w_full()
                .h_10()
                .items_center()
                .justify_between()
                .pr_3()
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().size_2().flex_none().rounded_full().bg(brand_pink()))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Double Love Studio"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted_foreground)
                                .child(fixtures.project_name),
                        )
                        .child(
                            div()
                                .h_5()
                                .px_1p5()
                                .rounded_sm()
                                .bg(with_alpha(brand_pink(), 0.14))
                                .text_xs()
                                .text_color(brand_pink())
                                .child(fixtures.episode_label),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w(px(176.))
                                .h_7()
                                .px_2()
                                .rounded_md()
                                .bg(theme.muted)
                                .text_xs()
                                .text_color(muted_foreground)
                                .child("搜索片段…"),
                        )
                        .child(
                            Button::new("import")
                                .outline()
                                .small()
                                .label("导入…")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_notice("导入向导将在后续迭代接入真实解析", cx);
                                })),
                        )
                        .child(
                            div()
                                .id("export-button")
                                .h_7()
                                .px_3()
                                .rounded_md()
                                .bg(brand_pink())
                                .hover(|style| style.bg(with_alpha(brand_pink(), 0.85)))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(hsla(0., 0., 1., 1.))
                                .child("导出 Premiere XML")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if export_blocked {
                                        this.set_notice(
                                            "⛔ 导出被 1 条错误诊断阻断：SHOTTAKE_INVALID（c21）",
                                            cx,
                                        );
                                    } else {
                                        this.set_notice("导出预演属后续迭代", cx);
                                    }
                                })),
                        ),
                ),
        )
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let fixtures = self.fixtures();
        let ignored = fixtures
            .clips
            .iter()
            .filter(|clip| clip.status == ClipStatus::Ignored)
            .count();

        let collections: [(&str, usize, Hsla, bool); 6] = [
            (
                "全部片段",
                fixtures.counts.total as usize,
                theme.foreground,
                true,
            ),
            (
                "已处理",
                fixtures.counts.processed as usize,
                theme.success,
                false,
            ),
            ("已忽略", ignored, theme.muted_foreground, false),
            (
                "已跳过",
                fixtures.counts.skipped as usize,
                theme.warning,
                false,
            ),
            ("失败", fixtures.counts.failed as usize, theme.danger, false),
            (
                "CSV 未匹配",
                fixtures.csv_unmatched as usize,
                rgb(0x8E4EC6).into(),
                false,
            ),
        ];

        v_flex()
            .w(px(208.))
            .flex_none()
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .p_3()
            .gap_4()
            .child(
                v_flex()
                    .gap_1()
                    .child(sidebar_section("项目", cx))
                    .child(sidebar_row("▾ 青空日记", None, false, cx))
                    .child(sidebar_row("　素材库", Some(21), false, cx))
                    .child(sidebar_row("　序列", Some(3), false, cx))
                    .child(sidebar_row("　导出", Some(1), false, cx)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(sidebar_section("智能集合", cx))
                    .children(collections.iter().map(|(label, count, color, selected)| {
                        sidebar_collection(label, *count, *color, *selected, cx)
                    })),
            )
    }

    fn render_center(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.render_preview(cx))
            .child(self.render_transport(cx))
            .child(self.render_scrub_strip(cx))
            .child(self.render_clip_table(cx))
    }

    fn render_preview(&self, cx: &Context<Self>) -> impl IntoElement {
        let selected = &self.fixtures().clips[self.selected_index()];
        let panel_black = hsla(0., 0., 0., 1.);
        let on_dark = hsla(0., 0., 0.96, 1.);
        let on_dark_muted = hsla(0., 0., 0.62, 1.);

        div()
            .h_56()
            .flex_none()
            .m_3()
            .mb_2()
            .rounded_md()
            .bg(panel_black)
            .v_flex()
            .items_center()
            .justify_center()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(on_dark)
                    .child(selected.new_name),
            )
            .child(
                div()
                    .text_sm()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(on_dark_muted)
                    .child(format!(
                        "{} ｜ {} ｜ 3840×2160 · 25fps",
                        selected.tc_in, selected.duration
                    )),
            )
            .child(
                div()
                    .mt_2()
                    .text_xs()
                    .text_color(on_dark_muted)
                    .child("预览画面占位 —— 真实解码属后续迭代"),
            )
    }

    fn render_transport(&self, cx: &Context<Self>) -> impl IntoElement {
        let muted_foreground = cx.theme().muted_foreground;

        h_flex()
            .h_9()
            .flex_none()
            .mx_3()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .id("prev-shot")
                    .text_xs()
                    .text_color(muted_foreground)
                    .hover(|style| style.text_color(cx.theme().foreground))
                    .child("⏮ 上一镜")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_notice("镜头导航属后续迭代", cx);
                    })),
            )
            .child(
                div()
                    .id("play-button")
                    .size_8()
                    .rounded_full()
                    .bg(select_blue())
                    .hover(|style| style.bg(with_alpha(select_blue(), 0.85)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(hsla(0., 0., 1., 1.))
                    .text_sm()
                    .child("▶")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_notice("播放预览属后续迭代", cx);
                    })),
            )
            .child(
                div()
                    .id("next-shot")
                    .text_xs()
                    .text_color(muted_foreground)
                    .hover(|style| style.text_color(cx.theme().foreground))
                    .child("⏭ 下一镜")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_notice("镜头导航属后续迭代", cx);
                    })),
            )
    }

    fn render_scrub_strip(&self, cx: &Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let thumb_bg = hsla(0., 0., 0.14, 1.);

        div()
            .h_12()
            .flex_none()
            .mx_3()
            .mb_2()
            .relative()
            .child(
                h_flex()
                    .h_full()
                    .w_full()
                    .gap_1()
                    .children((0..12).map(|_| {
                        div()
                            .flex_1()
                            .h_full()
                            .rounded_sm()
                            .bg(thumb_bg)
                            .border_1()
                            .border_color(border)
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(0.35))
                    .w(px(2.))
                    .bg(playhead_red()),
            )
    }

    fn render_clip_table(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let selected = self.selected_index();

        let header_cell = |label: &'static str| {
            div()
                .text_xs()
                .text_color(theme.table_head_foreground)
                .child(label)
        };

        let mut header = h_flex()
            .h_8()
            .flex_none()
            .px_2()
            .items_center()
            .gap_2()
            .bg(theme.table_head)
            .border_b_1()
            .border_color(theme.table_row_border);
        for (label, width) in CLIP_COLUMNS {
            header = header.child(div().w(width).flex_none().child(header_cell(label)));
        }

        let rows = div()
            .id("clip-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(
                self.fixtures()
                    .clips
                    .iter()
                    .enumerate()
                    .map(|(index, clip)| self.render_clip_row(index, clip, index == selected, cx)),
            );

        v_flex()
            .flex_1()
            .min_h_0()
            .mx_3()
            .mb_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.table)
            .overflow_hidden()
            .child(header)
            .child(rows)
    }

    fn render_clip_row(
        &self,
        index: usize,
        clip: &FixtureClip,
        selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let ignored = clip.status == ClipStatus::Ignored;
        let foreground = if ignored {
            theme.muted_foreground
        } else {
            theme.foreground
        };
        let selection_bg = with_alpha(select_blue(), 0.16);

        let rating_chip = match clip.rating {
            Rating::None => div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child("—"),
            rating => {
                let color = match rating {
                    Rating::Ok => theme.success,
                    Rating::Keep => select_blue(),
                    Rating::Ng => theme.danger,
                    Rating::None => unreachable!(),
                };
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(with_alpha(color, 0.14))
                    .text_xs()
                    .text_color(color)
                    .child(rating.label())
            }
        };

        let mut row = h_flex()
            .id(("clip-row", index))
            .h_8()
            .px_2()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(foreground)
            .border_b_1()
            .border_color(theme.table_row_border)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_clip(index, cx);
            }));

        if selected {
            row = row.bg(selection_bg);
        } else {
            row = row.hover(|style| style.bg(theme.table_hover));
        }

        row.child(
            div().w(CLIP_COLUMNS[0].1).flex_none().child(
                div()
                    .size_1p5()
                    .rounded_full()
                    .bg(status_color(clip.status, cx)),
            ),
        )
        .child(
            div()
                .w(CLIP_COLUMNS[1].1)
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .truncate()
                .child(clip.new_name),
        )
        .child(
            div()
                .w(CLIP_COLUMNS[2].1)
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(theme.muted_foreground)
                .truncate()
                .child(clip.source_name),
        )
        .child(div().w(CLIP_COLUMNS[3].1).flex_none().child(clip.scene))
        .child(div().w(CLIP_COLUMNS[4].1).flex_none().child(clip.shot))
        .child(div().w(CLIP_COLUMNS[5].1).flex_none().child(clip.take))
        .child(div().w(CLIP_COLUMNS[6].1).flex_none().child(clip.camera))
        .child(div().w(CLIP_COLUMNS[7].1).flex_none().child(rating_chip))
        .child(
            div()
                .w(CLIP_COLUMNS[8].1)
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .child(clip.tc_in),
        )
        .child(
            div()
                .w(CLIP_COLUMNS[9].1)
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .child(clip.duration),
        )
        .child(
            h_flex()
                .w(CLIP_COLUMNS[10].1)
                .flex_none()
                .gap_1()
                .items_center()
                .when(clip.from_csv, |this| {
                    this.child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(with_alpha(cx.theme().accent, 0.5))
                            .text_xs()
                            .text_color(cx.theme().foreground)
                            .child("CSV"),
                    )
                })
                .when(!clip.note.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(if clip.status == ClipStatus::Failed {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .truncate()
                            .child(clip.note),
                    )
                }),
        )
    }

    fn render_inspector(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let fixtures = self.fixtures();
        let selected = &fixtures.clips[self.selected_index()];
        let diagnostics: Vec<_> = fixtures
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.object_id.as_deref() == Some(selected.id))
            .collect();

        div()
            .id("inspector")
            .w_80()
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.border)
            .p_3()
            .v_flex()
            .gap_3()
            .child(self.render_metadata_card(selected, cx))
            .child(self.render_source_card(selected, cx))
            .child(self.render_diagnostics_card(&diagnostics, cx))
            .child(self.render_actions_card(cx))
            .child(self.render_revisions_card(cx))
    }

    fn render_metadata_card(&self, clip: &FixtureClip, cx: &Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        card("片段元数据", cx).child(
            v_flex()
                .gap_1()
                .child(kv_row("新名称", clip.new_name, true, &mono, cx))
                .child(kv_row("源文件", clip.source_name, true, &mono, cx))
                .child(kv_row("场景", clip.scene, false, &mono, cx))
                .child(kv_row("镜号", clip.shot, false, &mono, cx))
                .child(kv_row("镜次", clip.take, false, &mono, cx))
                .child(kv_row("机位", clip.camera, false, &mono, cx))
                .child(kv_row("评分", clip.rating.label(), false, &mono, cx))
                .child(kv_row("入点", clip.tc_in, true, &mono, cx))
                .child(kv_row("时长", clip.duration, true, &mono, cx))
                .child(kv_row(
                    "来源",
                    if clip.from_csv {
                        "XML + CSV 场记单"
                    } else {
                        "仅 XML"
                    },
                    false,
                    &mono,
                    cx,
                ))
                .child(kv_row("状态", status_label(clip.status), false, &mono, cx)),
        )
    }

    fn render_source_card(&self, clip: &FixtureClip, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted_foreground = theme.muted_foreground;

        let mut rows = v_flex()
            .gap_1()
            .child(source_row(
                "场景",
                clip.scene,
                if clip.from_csv { clip.scene } else { "—" },
                cx,
            ))
            .child(source_row(
                "Episode",
                "—",
                if clip.from_csv {
                    "02（采用）"
                } else {
                    "—"
                },
                cx,
            ))
            .child(source_row(
                "评分",
                clip.rating.label(),
                if clip.from_csv {
                    clip.rating.label()
                } else {
                    "—"
                },
                cx,
            ));

        if !clip.from_csv {
            rows = rows.child(
                div()
                    .text_xs()
                    .text_color(theme.warning)
                    .child("⚠ 该片段未匹配 CSV，按无 CSV 格式命名"),
            );
        } else if clip.note.is_empty() {
            rows = rows.child(
                div()
                    .text_xs()
                    .text_color(muted_foreground)
                    .child("XML 与 CSV 取值一致"),
            );
        }

        card("来源对照（XML ｜ CSV）", cx).child(rows)
    }

    fn render_diagnostics_card(
        &self,
        diagnostics: &[&double_love_engine::Diagnostic],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();

        let body = if diagnostics.is_empty() {
            v_flex().child(
                div()
                    .text_xs()
                    .text_color(theme.success)
                    .child("✓ 该片段无诊断"),
            )
        } else {
            v_flex()
                .gap_2()
                .children(diagnostics.iter().map(|diagnostic| {
                    let color = match diagnostic.level {
                        double_love_engine::DiagnosticLevel::Error => theme.danger,
                        double_love_engine::DiagnosticLevel::Warning => theme.warning,
                        double_love_engine::DiagnosticLevel::Info => theme.info,
                    };
                    v_flex()
                        .gap_0p5()
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(div().size_1p5().rounded_full().bg(color))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(diagnostic.code.clone()),
                                )
                                .when(diagnostic.blocks_export, |this| {
                                    this.child(
                                        div()
                                            .px_1()
                                            .rounded_sm()
                                            .bg(with_alpha(theme.danger, 0.14))
                                            .text_xs()
                                            .text_color(theme.danger)
                                            .child("⛔ 阻断导出"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(diagnostic.cause.clone()),
                        )
                }))
        };

        card("诊断", cx).child(body)
    }

    fn render_actions_card(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let export_blocked = self.fixtures().export_blocked;

        card("操作", cx).child(
            v_flex()
                .gap_2()
                .child(
                    Button::new("re-preview")
                        .outline()
                        .small()
                        .label("重新预演")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_notice("重新预演属后续迭代（需接入真实 Engine 操作）", cx);
                        })),
                )
                .child(
                    div()
                        .id("inspector-export")
                        .h_8()
                        .rounded_md()
                        .bg(brand_pink())
                        .hover(|style| style.bg(with_alpha(brand_pink(), 0.85)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hsla(0., 0., 1., 1.))
                        .child("导出 Premiere XML")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if export_blocked {
                                this.set_notice(
                                    "⛔ 导出被 1 条错误诊断阻断：SHOTTAKE_INVALID（c21）",
                                    cx,
                                );
                            } else {
                                this.set_notice("导出预演属后续迭代", cx);
                            }
                        })),
                )
                .when(export_blocked, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.danger)
                            .child("⛔ 导出被 1 条错误诊断阻断，需先在诊断中处理 c21"),
                    )
                }),
        )
    }

    fn render_revisions_card(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        card("版本历史", cx).child(
            v_flex().gap_1p5().children(
                self.fixtures()
                    .revisions
                    .iter()
                    .map(|entry| {
                        v_flex()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(
                                        format!("r{} · {}", entry.revision, entry.operation),
                                    ))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(entry.committed_at),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(entry.summary),
                            )
                    })
                    .collect::<Vec<_>>(),
            ),
        )
    }

    fn render_timeline(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        v_flex()
            .h_32()
            .flex_none()
            .border_t_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .h_4()
                    .flex_none()
                    .px_2()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .children(
                        [
                            "00:00", "00:20", "00:40", "01:00", "01:20", "01:40", "02:00",
                        ]
                        .iter()
                        .map(|label| div().child(*label)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(
                        canvas(
                            |_, _, _| (),
                            |bounds, _, window, cx| paint_sync_timeline(bounds, window, cx),
                        )
                        .size_full(),
                    )
                    .child(timeline_track_label("A", 42., cx))
                    .child(timeline_track_label("B", 66., cx))
                    .child(timeline_track_label("音频", 90., cx)),
            )
    }

    fn render_health(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let counts = &self.fixtures().counts;

        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .text_color(theme.danger)
                    .child(format!("⛔ {} 错误", counts.failed)),
            )
            .child(div().text_color(theme.warning).child("2 警告"))
            .child(div().child("3 提示"))
            .child(div().child(format!(
                "｜共 {} 片段：{} 处理 · 3 忽略 · {} 跳过 · {} 失败",
                counts.total, counts.processed, counts.skipped, counts.failed
            )))
    }
}

/// 素材表列定义（标签 + 固定宽度）。
const CLIP_COLUMNS: [(&str, Pixels); 11] = [
    ("状态", px(28.)),
    ("新名称", px(150.)),
    ("源文件", px(170.)),
    ("场", px(44.)),
    ("镜", px(36.)),
    ("次", px(48.)),
    ("机位", px(36.)),
    ("评分", px(44.)),
    ("入点", px(88.)),
    ("时长", px(88.)),
    ("备注", px(180.)),
];

fn render_notice(notice: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .h_7()
        .flex_none()
        .px_3()
        .flex()
        .items_center()
        .bg(with_alpha(theme.info, 0.12))
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.foreground)
        .child(format!("ℹ︎ {notice}"))
}

fn sidebar_section(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

fn sidebar_row(
    label: &'static str,
    count: Option<u64>,
    selected: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .h_6()
        .px_2()
        .items_center()
        .justify_between()
        .rounded_sm()
        .text_sm()
        .when(selected, |this| this.bg(theme.sidebar_accent))
        .child(div().child(label))
        .when_some(count, |this, count| {
            this.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(count.to_string()),
            )
        })
}

fn sidebar_collection(
    label: &'static str,
    count: usize,
    color: Hsla,
    selected: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .h_6()
        .px_2()
        .items_center()
        .gap_2()
        .rounded_sm()
        .text_sm()
        .when(selected, |this| {
            this.bg(with_alpha(select_blue(), 0.14))
                .text_color(select_blue())
        })
        .child(div().size_1p5().flex_none().rounded_full().bg(color))
        .child(div().flex_1().child(label))
        .child(
            div()
                .text_xs()
                .text_color(if selected {
                    select_blue()
                } else {
                    theme.muted_foreground
                })
                .child(count.to_string()),
        )
}

fn card(title: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.group_box)
        .p_3()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
}

fn kv_row(
    label: &'static str,
    value: &str,
    mono: bool,
    mono_family: &SharedString,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let mut value_el = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_xs()
        .child(value.to_string());
    if mono {
        value_el = value_el.font_family(mono_family.clone());
    }
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .w_16()
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(value_el)
}

fn source_row(label: &'static str, xml: &str, csv: &str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let matched =
        xml == csv || csv.ends_with("（采用）") || xml == csv.trim_end_matches("（采用）");
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .w_16()
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(div().w_16().flex_none().text_xs().child(xml.to_string()))
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(if matched {
                    theme.foreground
                } else {
                    theme.warning
                })
                .child(csv.to_string()),
        )
}

fn timeline_track_label(label: &'static str, top: f32, cx: &App) -> impl IntoElement {
    div()
        .absolute()
        .left(px(10.))
        .top(px(top))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// 伪随机但确定性的波形高度（0..1）。
fn pseudo_random(index: usize) -> f32 {
    let value = (index as f64 * 12.9898).sin() * 43758.5453;
    (value - value.floor()) as f32
}

/// 底部对位时间线的 custom paint：波形 + A/B 机轨 + 音频轨 + 红播放头。
fn paint_sync_timeline(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let theme = cx.theme();
    let x0 = bounds.left() + px(28.);
    let width: f32 = (bounds.size.width - px(40.)).into();
    let width = width.max(0.);
    let px_per_sec = width / TIMELINE_TOTAL_SEC;
    let top: f32 = bounds.top().into();

    let clip_rect =
        |start: f32, duration: f32, y: f32, height: f32, color: Hsla, window: &mut Window| {
            let mut quad = fill(
                Bounds {
                    origin: point(x0 + px(start * px_per_sec), px(top + y)),
                    size: gpui::size(px((duration * px_per_sec).max(2.)), px(height)),
                },
                color,
            );
            quad.corner_radii = Corners::all(px(3.));
            window.paint_quad(quad);
        };

    // 波形条
    let wave_color = with_alpha(theme.muted_foreground, 0.45);
    let bar_count = 160;
    for index in 0..bar_count {
        let bar_width = width / bar_count as f32;
        let bar_height = 4. + 24. * pseudo_random(index);
        let x = x0 + px(index as f32 * bar_width);
        let y = px(top + 4. + (32. - bar_height) / 2.);
        window.paint_quad(fill(
            Bounds {
                origin: point(x, y),
                size: gpui::size(px((bar_width * 0.55).max(1.)), px(bar_height)),
            },
            wave_color,
        ));
    }

    // 波形中线
    window.paint_quad(fill(
        Bounds {
            origin: point(x0, px(top + 19.5)),
            size: gpui::size(px(width), px(0.5)),
        },
        with_alpha(theme.muted_foreground, 0.25),
    ));

    // A 机轨（蓝）
    for (start, duration) in A_TRACK_CLIPS {
        clip_rect(
            start,
            duration,
            42.,
            18.,
            with_alpha(select_blue(), 0.7),
            window,
        );
    }
    // B 机轨（青）
    for (start, duration) in B_TRACK_CLIPS {
        clip_rect(
            start,
            duration,
            66.,
            18.,
            with_alpha(track_teal(), 0.7),
            window,
        );
    }
    // 音频轨（绿）
    for (start, duration) in AUDIO_TRACK_CLIPS {
        clip_rect(
            start,
            duration,
            90.,
            12.,
            with_alpha(audio_green(), 0.75),
            window,
        );
    }

    // 红色播放头
    let playhead_x = x0 + px(width * PLAYHEAD_POSITION);
    window.paint_quad(fill(
        Bounds {
            origin: point(playhead_x, px(top)),
            size: gpui::size(px(1.5), px(108.)),
        },
        playhead_red(),
    ));
    window.paint_quad(fill(
        Bounds {
            origin: point(playhead_x - px(4.), px(top)),
            size: gpui::size(px(9.5), px(6.)),
        },
        playhead_red(),
    ));
}

const TIMELINE_TOTAL_SEC: f32 = 120.;
const PLAYHEAD_POSITION: f32 = 0.35;
const A_TRACK_CLIPS: [(f32, f32); 6] = [
    (0., 14.),
    (16., 9.),
    (31., 12.),
    (52., 18.),
    (75., 11.),
    (95., 15.),
];
const B_TRACK_CLIPS: [(f32, f32); 4] = [(4., 10.), (28., 8.), (58., 14.), (86., 9.)];
const AUDIO_TRACK_CLIPS: [(f32, f32); 4] = [(0., 30.), (34., 26.), (64., 30.), (98., 18.)];
