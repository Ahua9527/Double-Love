import contextlib
import io
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path

from double_love_asr import modelscope_download


class ModelScopeDownloadTests(unittest.TestCase):
    def request(self, directory: str) -> dict:
        return {
            "repo_id": "mlx-community/fixture",
            "revision": "a" * 40,
            "local_dir": directory,
            "files": ["config.json"],
            "user_agent": "double-love-studio/0.2.0",
        }

    def test_request_rejects_floating_revision_and_unsafe_file(self):
        with tempfile.TemporaryDirectory() as directory:
            request = self.request(directory)
            request["revision"] = "master"
            with self.assertRaises(modelscope_download.DownloadRequestError):
                modelscope_download._request(request)

            request = self.request(directory)
            request["files"] = ["../example.py"]
            with self.assertRaises(modelscope_download.DownloadRequestError):
                modelscope_download._request(request)

    def test_sdk_callback_emits_jsonl_without_local_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            request = modelscope_download._request(self.request(directory))
            original = {
                name: sys.modules.get(name)
                for name in ["modelscope", "modelscope.hub", "modelscope.hub.snapshot_download", "modelscope_hub"]
            }

            class ProgressCallback:
                def __init__(self, filename: str, file_size: int):
                    self.filename = filename
                    self.file_size = file_size

                def update(self, _size: int) -> None:
                    pass

                def end(self) -> None:
                    pass

            def snapshot_download(**kwargs):
                callback = kwargs["progress_callbacks"][0]("config.json", 5)
                callback.update(5)
                callback.end()
                path = Path(kwargs["local_dir"]) / "config.json"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"hello")
                return kwargs["local_dir"]

            modelscope = types.ModuleType("modelscope")
            hub = types.ModuleType("modelscope.hub")
            snapshot = types.ModuleType("modelscope.hub.snapshot_download")
            modelscope_hub = types.ModuleType("modelscope_hub")
            snapshot.snapshot_download = snapshot_download
            modelscope_hub.ProgressCallback = ProgressCallback
            sys.modules["modelscope"] = modelscope
            sys.modules["modelscope.hub"] = hub
            sys.modules["modelscope.hub.snapshot_download"] = snapshot
            sys.modules["modelscope_hub"] = modelscope_hub
            output = io.StringIO()
            try:
                with contextlib.redirect_stdout(output):
                    modelscope_download._download(request)
            finally:
                for name, module in original.items():
                    if module is None:
                        sys.modules.pop(name, None)
                    else:
                        sys.modules[name] = module

            events = [json.loads(line) for line in output.getvalue().splitlines()]
            self.assertEqual(events[0]["event"], "started")
            self.assertEqual(events[-1]["event"], "completed")
            self.assertTrue(any(event["event"] == "progress" for event in events))
            self.assertNotIn(directory, output.getvalue())
            self.assertEqual((Path(directory) / "config.json").read_bytes(), b"hello")


if __name__ == "__main__":
    unittest.main()
