"""受管 ModelScope 下载器。

这个模块只由 desktop host 启动。stdin/stdout 使用 JSONL，避免解析 SDK CLI
的人类文本；SDK 负责带 Range 的分片续传，Rust 在完成后仍会用内置清单做
大小、SHA-256 和原子安装校验。这里绝不导入或执行模型仓库内的 Python 文件。
"""

from __future__ import annotations

import json
import re
import sys
import threading
from pathlib import Path, PurePosixPath
from typing import Any


_REPO_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*$")
_REVISION = re.compile(r"^[0-9a-f]{40}$")
_LOCK = threading.Lock()


class DownloadRequestError(ValueError):
    """The host supplied an invalid or unsafe download request."""


def _emit(event: str, **fields: Any) -> None:
    with _LOCK:
        print(json.dumps({"event": event, **fields}, ensure_ascii=False), flush=True)


def _safe_relative(value: object) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise DownloadRequestError("模型文件清单包含不安全路径。")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise DownloadRequestError("模型文件清单包含不安全路径。")
    return value


def _request(raw: object) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise DownloadRequestError("模型下载请求必须是 JSON 对象。")
    repo_id = raw.get("repo_id")
    revision = raw.get("revision")
    local_dir = raw.get("local_dir")
    files = raw.get("files")
    user_agent = raw.get("user_agent")
    if not isinstance(repo_id, str) or not _REPO_ID.fullmatch(repo_id):
        raise DownloadRequestError("模型仓库标识无效。")
    if not isinstance(revision, str) or not _REVISION.fullmatch(revision):
        raise DownloadRequestError("模型 revision 必须是固定的 40 位提交。")
    if not isinstance(local_dir, str) or not Path(local_dir).is_absolute():
        raise DownloadRequestError("模型 staging 目录必须是绝对路径。")
    if not isinstance(files, list) or not files:
        raise DownloadRequestError("模型文件白名单不能为空。")
    safe_files = [_safe_relative(item) for item in files]
    if len(set(safe_files)) != len(safe_files):
        raise DownloadRequestError("模型文件白名单不能重复。")
    if not isinstance(user_agent, str) or not user_agent.startswith("double-love-studio/"):
        raise DownloadRequestError("模型下载客户端标识无效。")
    if len(user_agent) > 160 or any(ord(char) < 32 or ord(char) > 126 for char in user_agent):
        raise DownloadRequestError("模型下载客户端标识无效。")
    return {
        "repo_id": repo_id,
        "revision": revision,
        "local_dir": local_dir,
        "files": safe_files,
        "user_agent": user_agent,
    }


def _download(request: dict[str, Any]) -> None:
    try:
        from modelscope.hub.snapshot_download import snapshot_download
        from modelscope_hub import ProgressCallback
    except Exception as error:  # pragma: no cover - integration/runtime path
        raise RuntimeError("ModelScope 下载组件不可用；请重新安装本机模型运行时。") from error

    allowed = set(request["files"])
    totals: dict[str, int] = {}
    downloaded: dict[str, int] = {}
    progress_lock = threading.Lock()

    class JsonlProgress(ProgressCallback):
        def __init__(self, filename: str, file_size: int):
            self.filename = filename.replace("\\", "/")
            self.file_size = max(0, int(file_size))
            if self.filename in allowed:
                with progress_lock:
                    totals[self.filename] = self.file_size
                    downloaded.setdefault(self.filename, 0)

        def update(self, size: int) -> None:
            if self.filename not in allowed:
                return
            with progress_lock:
                previous = downloaded.get(self.filename, 0)
                # SDK callback reports resume bytes once, then incremental chunks. Never let a
                # malformed callback claim more than the immutable manifest's file size.
                next_value = min(self.file_size, max(previous, previous + max(0, int(size))))
                downloaded[self.filename] = next_value
                completed = sum(downloaded.values())
                total = sum(totals.values())
            _emit(
                "progress",
                current_file=self.filename,
                bytes_downloaded=completed,
                bytes_total=total,
                file_bytes_downloaded=next_value,
                file_bytes_total=self.file_size,
            )

        def end(self) -> None:
            if self.filename in allowed:
                with progress_lock:
                    downloaded[self.filename] = self.file_size

    _emit("started", repo_id=request["repo_id"], revision=request["revision"])
    Path(request["local_dir"]).mkdir(parents=True, exist_ok=True)
    snapshot_download(
        repo_id=request["repo_id"],
        revision=request["revision"],
        local_dir=request["local_dir"],
        allow_patterns=request["files"],
        user_agent=request["user_agent"],
        max_workers=2,
        progress_callbacks=[JsonlProgress],
    )
    _emit("completed")


def main() -> int:
    try:
        line = sys.stdin.readline()
        if not line:
            raise DownloadRequestError("缺少模型下载请求。")
        request = _request(json.loads(line))
        _download(request)
        return 0
    except DownloadRequestError as error:
        _emit("error", code="MODELSCOPE_REQUEST_INVALID", message=str(error))
    except json.JSONDecodeError:
        _emit("error", code="MODELSCOPE_REQUEST_INVALID", message="模型下载请求不是合法 JSON。")
    except Exception:
        # Keep raw URLs, absolute staging paths and SDK internals out of the renderer.
        _emit("error", code="MODELSCOPE_DOWNLOAD_FAILED", message="ModelScope 下载失败，请检查网络后重试。")
    return 1


if __name__ == "__main__":  # pragma: no cover - subprocess entrypoint
    raise SystemExit(main())
