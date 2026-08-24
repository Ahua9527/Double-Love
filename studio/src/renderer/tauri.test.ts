import { describe, expect, it } from "vitest";
import {
    normalizeDoctorReport,
    normalizeModelDescriptor,
    normalizeModelProgress,
} from "./platform/normalize";

describe("应用级桌面 DTO normalize", () => {
    it("把 Rust 的 descriptor + installation 快照转成 UI 模型行", () => {
        const model = normalizeModelDescriptor({
            descriptor: {
                id: "qwen3-asr-0.6b-4bit",
                display_name: "Qwen3 ASR 0.6B · 4-bit",
                component: "asr",
                download_source: "modelscope",
                revision: "fixed-revision",
                files: [{ size_bytes: 1024 }],
                dependencies: [],
                license: "Apache-2.0",
            },
            installation: {
                model_id: "qwen3-asr-0.6b-4bit",
                revision: "fixed-revision",
                state: "paused",
                bytes_downloaded: 256,
                bytes_total: 1024,
                last_error_message: null,
            },
        });
        expect(model.label).toBe("Qwen3 ASR 0.6B · 4-bit");
        expect(model.state).toBe("paused");
        expect(model.downloaded_bytes).toBe(256);
        expect(model.size_bytes).toBe(1024);
        expect(model.download_source).toBe("modelscope");
    });

    it("保留下载事件的进度字段，不把暂停或失败伪装成完成", () => {
        const progress = normalizeModelProgress({
            model_id: "qwen3-asr-1.7b-8bit",
            state: "verifying",
            bytes_downloaded: 10,
            bytes_total: 20,
        });
        expect(progress.state).toBe("verifying");
        expect(progress.completed_bytes).toBe(10);
    });

    it("把 Rust 诊断布尔状态映射为可读报告", () => {
        const report = normalizeDoctorReport({
            architecture: "arm64",
            ffmpeg_available: true,
            libass_available: false,
            model_checks: [
                {
                    model_id: "qwen3-asr-0.6b-4bit",
                    state: "corrupt",
                    error_code: "SHA256_MISMATCH",
                },
            ],
        });
        expect(report.app_version).toBe("0.2.0");
        expect(report.ffmpeg).toBe("可用");
        expect(report.libass).toBe("不可用");
        expect(report.model_integrity[0]?.state).toBe("corrupt");
    });

    it("保留 App 内置 runtime 的结构化诊断结果", () => {
        const report = normalizeDoctorReport({
            capability_checks: [
                {
                    id: "media.h264_encoder",
                    status: "blocked",
                    detail: "App 内置 ffmpeg 的 H.264 编码器不可用。",
                    suggested_action: "请重新安装完整的 Double Love Studio。",
                },
            ],
        });
        expect(report.capability_checks).toEqual([
            {
                id: "media.h264_encoder",
                status: "blocked",
                detail: "App 内置 ffmpeg 的 H.264 编码器不可用。",
                suggested_action: "请重新安装完整的 Double Love Studio。",
            },
        ]);
    });
});
