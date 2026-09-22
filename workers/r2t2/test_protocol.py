"""Fault probes that need neither model weights nor GPU access."""
import json
from pathlib import Path
import subprocess
import unittest

WORKER = Path(__file__).resolve().parents[2] / "src-tauri/bundle/r2t2/blabber-r2t2-worker"


class ProtocolFailures(unittest.TestCase):
    def reject(self, request):
        run = subprocess.run([str(WORKER)], input=request, text=True, capture_output=True, timeout=10)
        self.assertNotEqual(run.returncode, 0)
        messages = [json.loads(line) for line in run.stdout.splitlines()]
        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["type"], "error")
        self.assertEqual(messages[0]["code"], "R2T2_PROTOCOL")
        self.assertNotIn("private test text", run.stderr)

    def test_malformed_and_truncated_json(self):
        self.reject("{broken}\n")
        self.reject('{"private test text":')
        self.reject('{"version":1,"type":"start"} trailing garbage\n')

    def test_unknown_version_is_rejected_before_loading(self):
        self.reject(json.dumps(dict(version=9, type="start", sessionId="test", sequence=0)) + "\n")

    def test_missing_session_and_unordered_start(self):
        self.reject(json.dumps(dict(version=1, type="audio", sessionId="test", sequence=1)) + "\n")
        self.reject(json.dumps(dict(version=1, type="start", sessionId="test", sequence=2)) + "\n")

    def test_oversized_frame_is_bounded(self):
        self.reject("x" * (1024 * 1024 + 1))

    def test_clean_idle_eof_releases_process(self):
        run = subprocess.run([str(WORKER)], input="", capture_output=True, timeout=10)
        self.assertEqual(run.returncode, 0)
        self.assertEqual(run.stdout, b"")


if __name__ == "__main__":
    unittest.main()
