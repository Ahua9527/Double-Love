"""Double Love ASR sidecar 入口：python -m double_love_asr

stdin 每行一个命令 JSON；stdout 每行一个事件 JSON；stderr 只写日志。
DOUBLELOVE_ASR_MOCK=1 → 使用确定性 mock（测试/开发自举，不加载模型）。

命令：hello / transcribe / cancel
事件：ready / progress / words / done / cancelled / error
"""

import json
import os
import sys
import threading

from . import mock, pipeline, protocol, transcriber


def _run(engine_run, cmd: dict, cancel: threading.Event) -> None:
    task_id = cmd.get("task_id", "")
    try:
        engine_run(cmd, cancel, protocol.emit)
    except pipeline.AsrError as error:
        protocol.emit(
            {
                "event": "error",
                "task_id": task_id,
                "code": error.code,
                "message": str(error),
                "fatal": error.fatal,
            }
        )
    except Exception as error:  # 协议不允许静默崩溃：任何意外都上报
        protocol.emit(
            {
                "event": "error",
                "task_id": task_id,
                "code": "ASR_INTERNAL",
                "message": repr(error),
                "fatal": True,
            }
        )


def main() -> int:
    use_mock = os.environ.get("DOUBLELOVE_ASR_MOCK") == "1"
    engine_run = mock.run if use_mock else transcriber.run
    worker: threading.Thread | None = None
    cancel = threading.Event()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            cmd = json.loads(line)
        except json.JSONDecodeError as error:
            protocol.emit(
                {
                    "event": "error",
                    "task_id": None,
                    "code": "ASR_BAD_COMMAND",
                    "message": f"命令不是合法 JSON：{error}",
                    "fatal": False,
                }
            )
            continue

        kind = cmd.get("cmd")
        if kind == "hello":
            protocol.emit(
                {
                    "event": "ready",
                    "version": protocol.PROTOCOL_VERSION,
                    "pid": os.getpid(),
                    "mock": use_mock,
                }
            )
        elif kind == "transcribe":
            if worker is not None and worker.is_alive():
                protocol.emit(
                    {
                        "event": "error",
                        "task_id": cmd.get("task_id"),
                        "code": "ASR_BUSY",
                        "message": "已有转录任务在进行，请等待完成或先取消",
                        "fatal": False,
                    }
                )
                continue
            cancel.clear()
            worker = threading.Thread(
                target=_run, args=(engine_run, cmd, cancel), daemon=True
            )
            worker.start()
        elif kind == "cancel":
            cancel.set()
        else:
            protocol.emit(
                {
                    "event": "error",
                    "task_id": None,
                    "code": "ASR_BAD_COMMAND",
                    "message": f"未知命令：{kind}",
                    "fatal": False,
                }
            )
    # stdin 关闭 = 父进程退出；daemon worker 随之结束
    return 0


if __name__ == "__main__":
    sys.exit(main())
