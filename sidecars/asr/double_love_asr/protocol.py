"""JSONL 行协议常量与发送助手。每行一个 JSON，UTF-8，不转义非 ASCII。"""

import json
import sys

PROTOCOL_VERSION = 1


def emit(event: dict) -> None:
    sys.stdout.write(json.dumps(event, ensure_ascii=False) + "\n")
    sys.stdout.flush()
