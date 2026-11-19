import json
import tempfile
import threading
import time
import unittest
from http.client import HTTPConnection
from pathlib import Path
from unittest.mock import patch

from installer import launcher


class InstallerAPITest(unittest.TestCase):
    def setUp(self):
        self.server = launcher.InstallerServer(0)
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.headers = {"Origin": f"http://127.0.0.1:{self.server.port}",
                        "Authorization": "Bearer " + self.server.session}

    def tearDown(self):
        self.server.shutdown()
        self.thread.join(timeout=5)
        self.server.server_close()

    def request(self, path, data=None, headers=None):
        conn = HTTPConnection("127.0.0.1", self.server.port, timeout=3)
        try:
            conn.request("GET" if data is None else "POST", "/api/" + path,
                         None if data is None else json.dumps(data), headers={
                             "Content-Type": "application/json", **(self.headers if headers is None else headers)
                         })
            res = conn.getresponse()
            return res.status, json.load(res)
        finally:
            conn.close()

    def wait_for(self, state):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            code, body = self.request("status")
            self.assertEqual(code, 200)
            if body["state"] == state:
                return body
            time.sleep(.01)
        self.fail("Installer did not reach " + state)

    def test_api_requires_launcher_session_and_local_origin(self):
        self.assertEqual(self.request("session", headers={})[0], 401)
        self.assertEqual(self.request("session")[0], 200)
        self.assertEqual(self.request("close", {}, {**self.headers, "Origin": "null"})[0], 403)
        self.assertEqual(self.request("lan", {})[0], 409)

    def test_folder_picker_lists_directories_and_leaves_files_unread(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            (root / "Albums").mkdir()
            (root / ".private").mkdir()
            (root / "song.mp3").touch()
            code, result = self.request("folders", {"path": str(root)})
            self.assertEqual(code, 200)
            self.assertEqual(result["folders"], [{"name": "Albums", "path": str(root / "Albums")}])
            self.assertEqual(result["parent"], str(root.parent))
            self.assertEqual(self.request("folders", {"path": str(root / "absent")})[0], 400)

    def test_install_serializes_work_and_preserves_lan_retry(self):
        release = threading.Event()
        self.addCleanup(release.set)
        plan = launcher.engine.defaults()
        plan["access"] = "lan"
        fresh = {"url": "http://127.0.0.1:8880/setup", "directory": plan["directory"], "lanPending": True}

        def install(settings, report):
            self.assertEqual(settings, plan)
            report("download", "Téléchargement")
            release.wait(timeout=3)
            return fresh

        with patch.object(launcher.engine, "install", install):
            self.assertEqual(self.request("install", plan)[0], 202)
            self.assertEqual(self.request("install", plan)[0], 409)
            self.assertEqual(self.request("close", {})[0], 409)
            release.set()
            result = self.wait_for("complete")
            self.assertEqual(result["events"], [{"stage": "download", "message": "Téléchargement"}])
            self.assertEqual(result["result"], fresh)

        with patch.object(launcher.engine, "enable_lan", side_effect=ValueError("Terminez la configuration.")):
            self.assertEqual(self.request("lan", {})[0], 202)
            retry = self.wait_for("complete")["result"]
            self.assertTrue(retry["lanPending"])
            self.assertEqual(retry["lanError"], "Terminez la configuration.")

        enabled = {**fresh, "lanPending": False, "lanUrl": "http://music.local:8880/"}
        with patch.object(launcher.engine, "enable_lan", return_value=enabled):
            self.request("lan", {})
            self.assertEqual(self.wait_for("complete")["result"], enabled)
        self.assertEqual(self.request("close", {})[0], 200)
        self.thread.join(timeout=3)
        self.assertFalse(self.thread.is_alive())


if __name__ == "__main__":
    unittest.main()
