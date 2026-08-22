import type { CanvasSpec } from '../../../../bindings/CanvasSpec'
import type { FrameRate } from '../../../../bindings/FrameRate'
import type { SubtitleStyle } from '../../../../bindings/SubtitleStyle'
import type { RevisionHistoryEntry } from '../../../../bindings/RevisionHistoryEntry'
import { frameRateLabel, num } from '../utils'

interface ProjectSettingsProps {
  projectOpen: boolean
  canvas: CanvasSpec | null
  outputRate: FrameRate | null
  subtitleStyle: SubtitleStyle | null
  theme: 'light' | 'dark' | 'system'
  onThemeChange: (theme: 'light' | 'dark' | 'system') => void
  onCanvasSave: (canvas: CanvasSpec) => void
  onOutputRateSave: (rate: FrameRate | null) => void
  onStyleSave: (style: SubtitleStyle) => void
  history: RevisionHistoryEntry[]
  onRestoreRevision: (revision: number) => void
}

const OUTPUT_RATES: FrameRate[] = [
  'fps_24_ntsc',
  'fps_24',
  'fps_25',
  'fps_30_ntsc',
  'fps_30',
  'fps_50',
  'fps_60_ntsc',
  'fps_60',
]

function numberOr(value: string, fallback: number): number {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : fallback
}

export function ProjectSettings({ projectOpen, canvas, outputRate, subtitleStyle, theme, onThemeChange, onCanvasSave, onOutputRateSave, onStyleSave, history, onRestoreRevision }: ProjectSettingsProps) {
  if (!projectOpen) return <div className="studio-settings-loading">打开一个本地项目后，可以调整画布、字幕和项目历史。</div>
  if (!canvas || !subtitleStyle) return <div className="studio-settings-loading">正在读取项目设置…</div>
  return (
    <section className="studio-settings" aria-labelledby="settings-title">
      <header><h1 id="settings-title">项目设置</h1><p>画布和字幕样式在整个项目中保持一致。</p></header>
      <div className="studio-settings-section">
        <div><h2>外观</h2><p>默认使用亮色工作台；也可以跟随系统或切换为深色。</p></div>
        <div className="studio-setting-grid"><label>主题<select aria-label="应用主题" value={theme} onChange={(event) => onThemeChange(event.target.value as 'light' | 'dark' | 'system')}><option value="light">亮色</option><option value="dark">深色</option><option value="system">跟随系统</option></select></label></div>
      </div>
      <div className="studio-settings-section">
        <div><h2>统一画布</h2><p>所有主轨素材会按同一画布输出。首个版本不支持逐片段变换或关键帧。</p></div>
        <div className="studio-setting-grid">
          <label>宽度<input aria-label="画布宽度" type="number" defaultValue={num(canvas.width)} onBlur={(event) => onCanvasSave({ ...canvas, width: Math.max(2, Math.round(numberOr(event.target.value, num(canvas.width)))) as unknown as bigint })} /></label>
          <label>高度<input aria-label="画布高度" type="number" defaultValue={num(canvas.height)} onBlur={(event) => onCanvasSave({ ...canvas, height: Math.max(2, Math.round(numberOr(event.target.value, num(canvas.height)))) as unknown as bigint })} /></label>
          <label>背景色<input aria-label="画布背景色" defaultValue={canvas.background} onBlur={(event) => onCanvasSave({ ...canvas, background: event.target.value })} /></label>
          <label>适配方式<select aria-label="画布适配方式" value={canvas.fit} onChange={(event) => onCanvasSave({ ...canvas, fit: event.target.value as CanvasSpec['fit'] })}><option value="contain">完整显示</option><option value="cover">铺满裁切</option></select></label>
          <label>水平位置<input aria-label="画布水平位置" type="number" step="0.01" defaultValue={canvas.position_x} onBlur={(event) => onCanvasSave({ ...canvas, position_x: numberOr(event.target.value, canvas.position_x) })} /></label>
          <label>垂直位置<input aria-label="画布垂直位置" type="number" step="0.01" defaultValue={canvas.position_y} onBlur={(event) => onCanvasSave({ ...canvas, position_y: numberOr(event.target.value, canvas.position_y) })} /></label>
          <label>缩放<input aria-label="画布缩放" type="number" step="0.05" defaultValue={canvas.scale} onBlur={(event) => onCanvasSave({ ...canvas, scale: Math.max(0.1, numberOr(event.target.value, canvas.scale)) })} /></label>
          <label>旋转<input aria-label="画布旋转" type="number" step="1" defaultValue={canvas.rotation_degrees} onBlur={(event) => onCanvasSave({ ...canvas, rotation_degrees: numberOr(event.target.value, canvas.rotation_degrees) })} /></label>
          <label>不透明度<input aria-label="画布不透明度" type="number" min="0" max="1" step="0.05" defaultValue={canvas.opacity} onBlur={(event) => onCanvasSave({ ...canvas, opacity: Math.max(0, Math.min(1, numberOr(event.target.value, canvas.opacity))) })} /></label>
        </div>
      </div>
      <div className="studio-settings-section">
        <div><h2>输出帧率</h2><p>默认跟随主轨第一段素材；混合帧率素材会在编译时间线时精确转换到这里的输出帧率。</p></div>
        <div className="studio-setting-grid"><label>输出帧率<select aria-label="输出帧率" value={outputRate ?? 'auto'} onChange={(event) => onOutputRateSave(event.target.value === 'auto' ? null : event.target.value as FrameRate)}><option value="auto">跟随第一段主轨素材</option>{OUTPUT_RATES.map((rate) => <option key={rate} value={rate}>{frameRateLabel(rate)}</option>)}</select></label></div>
      </div>
      <div className="studio-settings-section">
        <div><h2>项目历史</h2><p>恢复会创建新的版本，不会改写已有记录。文字删减只会在当前转录版本仍匹配时恢复。</p></div>
        <div className="studio-history-list">
          {history.length === 0 ? <p>还没有可恢复的编辑记录。</p> : history.slice(0, 10).map((entry) => (
            <article key={num(entry.revision)}><div><strong>{entry.operation.replace(/_/g, ' ')}</strong><small>版本 {num(entry.revision)} · {entry.committed_at}</small></div><button type="button" disabled={!entry.restorable} onClick={() => onRestoreRevision(num(entry.revision))}>恢复</button></article>
          ))}
        </div>
      </div>
      <div className="studio-settings-section">
        <div><h2>项目级字幕样式</h2><p>应用预览、ASS 与烧录 MP4 完整保留。NLE 导出只承诺文字与时间。</p></div>
        <div className="studio-setting-grid">
          <label>字体<select aria-label="字幕字体" value={subtitleStyle.font_family} onChange={(event) => onStyleSave({ ...subtitleStyle, font_family: event.target.value })}><option>PingFang SC</option><option>Hiragino Sans GB</option><option>Helvetica Neue</option></select></label>
          <label>字重<select aria-label="字幕字重" value={num(subtitleStyle.font_weight)} onChange={(event) => onStyleSave({ ...subtitleStyle, font_weight: Number(event.target.value) as unknown as bigint })}><option value="400">常规</option><option value="500">中等</option><option value="600">半粗</option></select></label>
          <label>字号<input aria-label="字幕字号" type="number" defaultValue={subtitleStyle.font_size} onBlur={(event) => onStyleSave({ ...subtitleStyle, font_size: Math.max(12, numberOr(event.target.value, subtitleStyle.font_size)) })} /></label>
          <label>文字颜色<input aria-label="字幕文字颜色" defaultValue={subtitleStyle.text_color} onBlur={(event) => onStyleSave({ ...subtitleStyle, text_color: event.target.value })} /></label>
          <label>描边颜色<input aria-label="字幕描边颜色" defaultValue={subtitleStyle.outline_color} onBlur={(event) => onStyleSave({ ...subtitleStyle, outline_color: event.target.value })} /></label>
          <label>描边宽度<input aria-label="字幕描边宽度" type="number" step="0.5" defaultValue={subtitleStyle.outline_width} onBlur={(event) => onStyleSave({ ...subtitleStyle, outline_width: Math.max(0, numberOr(event.target.value, subtitleStyle.outline_width)) })} /></label>
          <label>阴影颜色<input aria-label="字幕阴影颜色" defaultValue={subtitleStyle.shadow_color} onBlur={(event) => onStyleSave({ ...subtitleStyle, shadow_color: event.target.value })} /></label>
          <label>阴影水平偏移<input aria-label="字幕阴影水平偏移" type="number" step="0.5" defaultValue={subtitleStyle.shadow_offset_x} onBlur={(event) => onStyleSave({ ...subtitleStyle, shadow_offset_x: numberOr(event.target.value, subtitleStyle.shadow_offset_x) })} /></label>
          <label>阴影垂直偏移<input aria-label="字幕阴影垂直偏移" type="number" step="0.5" defaultValue={subtitleStyle.shadow_offset_y} onBlur={(event) => onStyleSave({ ...subtitleStyle, shadow_offset_y: numberOr(event.target.value, subtitleStyle.shadow_offset_y) })} /></label>
          <label>阴影模糊<input aria-label="字幕阴影模糊" type="number" step="0.5" defaultValue={subtitleStyle.shadow_blur} onBlur={(event) => onStyleSave({ ...subtitleStyle, shadow_blur: Math.max(0, numberOr(event.target.value, subtitleStyle.shadow_blur)) })} /></label>
          <label>背景色<input aria-label="字幕背景色" defaultValue={subtitleStyle.background_color} onBlur={(event) => onStyleSave({ ...subtitleStyle, background_color: event.target.value })} /></label>
          <label>背景圆角<input aria-label="字幕背景圆角" type="number" step="1" defaultValue={subtitleStyle.background_radius} onBlur={(event) => onStyleSave({ ...subtitleStyle, background_radius: Math.max(0, numberOr(event.target.value, subtitleStyle.background_radius)) })} /></label>
          <label>背景水平内边距<input aria-label="字幕背景水平内边距" type="number" step="1" defaultValue={subtitleStyle.background_padding_x} onBlur={(event) => onStyleSave({ ...subtitleStyle, background_padding_x: Math.max(0, numberOr(event.target.value, subtitleStyle.background_padding_x)) })} /></label>
          <label>背景垂直内边距<input aria-label="字幕背景垂直内边距" type="number" step="1" defaultValue={subtitleStyle.background_padding_y} onBlur={(event) => onStyleSave({ ...subtitleStyle, background_padding_y: Math.max(0, numberOr(event.target.value, subtitleStyle.background_padding_y)) })} /></label>
          <label>水平位置<input aria-label="字幕水平位置" type="number" min="0" max="1" step="0.01" defaultValue={subtitleStyle.position_x} onBlur={(event) => onStyleSave({ ...subtitleStyle, position_x: Math.max(0, Math.min(1, numberOr(event.target.value, subtitleStyle.position_x))) })} /></label>
          <label>垂直位置<input aria-label="字幕垂直位置" type="number" min="0" max="1" step="0.01" defaultValue={subtitleStyle.position_y} onBlur={(event) => onStyleSave({ ...subtitleStyle, position_y: Math.max(0, Math.min(1, numberOr(event.target.value, subtitleStyle.position_y))) })} /></label>
          <label>最大宽度比例<input aria-label="字幕最大宽度比例" type="number" min="0.1" max="1" step="0.01" defaultValue={subtitleStyle.max_width_ratio} onBlur={(event) => onStyleSave({ ...subtitleStyle, max_width_ratio: Math.max(0.1, Math.min(1, numberOr(event.target.value, subtitleStyle.max_width_ratio))) })} /></label>
          <label>最大行数<input aria-label="字幕最大行数" type="number" min="1" max="4" defaultValue={num(subtitleStyle.max_lines)} onBlur={(event) => onStyleSave({ ...subtitleStyle, max_lines: Math.max(1, Math.round(numberOr(event.target.value, num(subtitleStyle.max_lines)))) as unknown as bigint })} /></label>
          <label>每行目标字数<input aria-label="每行目标字数" type="number" defaultValue={num(subtitleStyle.target_characters_per_line)} onBlur={(event) => onStyleSave({ ...subtitleStyle, target_characters_per_line: Math.max(4, Math.round(numberOr(event.target.value, num(subtitleStyle.target_characters_per_line)))) as unknown as bigint })} /></label>
          <label className="studio-checkbox-label"><input aria-label="显示说话人名称" type="checkbox" checked={subtitleStyle.show_speaker} onChange={(event) => onStyleSave({ ...subtitleStyle, show_speaker: event.target.checked })} />显示说话人名称</label>
        </div>
      </div>
    </section>
  )
}
