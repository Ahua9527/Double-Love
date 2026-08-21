"""Run with `python -m double_love_speaker`; stdout is the shared JSONL protocol."""

import json
import os
import sys
import threading

from . import engine, mock, protocol


def _run(cmd: dict, cancel: threading.Event) -> None:
    task_id = cmd.get("task_id", "")
    try:
        (mock.diarize if os.environ.get("DOUBLELOVE_SPEAKER_MOCK") == "1" else engine.diarize)(cmd, cancel, protocol.emit)
    except engine.SpeakerError as error:
        protocol.emit({"event": "error", "task_id": task_id, "code": error.code, "message": str(error), "fatal": error.fatal})
    except Exception:
        protocol.emit({"event": "error", "task_id": task_id, "code": "SPEAKER_INTERNAL", "message": "本地说话人后端发生未预期错误。", "fatal": True})


def main() -> int:
    worker: threading.Thread | None = None
    cancel = threading.Event()
    for raw in sys.stdin:
        try:
            cmd = json.loads(raw)
        except json.JSONDecodeError:
            protocol.emit({"event": "error", "task_id": None, "code": "SPEAKER_BAD_COMMAND", "message": "命令不是合法 JSON。", "fatal": False})
            continue
        kind = cmd.get("cmd")
        if kind == "hello":
            protocol.emit({"event": "ready", "version": protocol.PROTOCOL_VERSION, "pid": os.getpid(), "mock": os.environ.get("DOUBLELOVE_SPEAKER_MOCK") == "1"})
        elif kind == "diarize":
            if worker is not None and worker.is_alive():
                protocol.emit({"event": "error", "task_id": cmd.get("task_id"), "code": "SPEAKER_BUSY", "message": "已有说话人任务在进行。", "fatal": False})
                continue
            cancel.clear()
            worker = threading.Thread(target=_run, args=(cmd, cancel), daemon=True)
            worker.start()
        elif kind == "cancel":
            cancel.set()
        else:
            protocol.emit({"event": "error", "task_id": None, "code": "SPEAKER_BAD_COMMAND", "message": "未知命令。", "fatal": False})
    return 0


if __name__ == "__main__":
    sys.exit(main())
