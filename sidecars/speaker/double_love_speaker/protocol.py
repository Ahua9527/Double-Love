"""The shared, small JSONL protocol. stdout is reserved for machine-readable events."""

import json
import sys


PROTOCOL_VERSION = 1


def emit(event: dict) -> None:
    sys.stdout.write(json.dumps(event, ensure_ascii=False) + "\n")
    sys.stdout.flush()
