import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from installer import qnap


class QnapVariantTests(unittest.TestCase):
    def test_standalone_yaml_resolves_without_project_environment(self):
        secret = ''' leading space \\ " ' # ${UNEXPECTED} $$ \\ '''
        variables = {"JWT_SECRET": secret, "POSTGRES_PASSWORD": "test-db-password",
                     "SOUNDTIME_BIND_IP": "127.0.0.1"}
        services = {
            "postgres": {"image": "postgres", "environment": {
                "POSTGRES_PASSWORD": "${POSTGRES_PASSWORD:?}"}},
            "backend": {"image": "backend", "environment": {
                "JWT_SECRET": "${JWT_SECRET:?}", "DATABASE_URL": "db://user:${POSTGRES_PASSWORD:?}@postgres",
                "PREFIX": "music/$$USD", "EMPTY": ""}},
            "nginx": {"image": "nginx", "ports": [
                {"target": 80, "published": "8880", "protocol": "tcp",
                 "host_ip": "${SOUNDTIME_BIND_IP:-127.0.0.1}"}]},
        }
        model = {"name": "soundtime-test", "services": services}
        original = copy.deepcopy(model)
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp) / "Mixed Case $USD"
            files = qnap.variant({"directory": str(folder)}, model, variables)
            self.assertEqual(model, original)
            self.assertNotIn(variables["POSTGRES_PASSWORD"].encode(), files["qnap-compose.yaml"])
            self.assertNotIn(secret.encode(), files["qnap-compose.yaml"])
            config = json.loads(files["qnap-compose.yaml"])
            self.assertNotIn("environment", config["services"]["backend"])
            self.assertEqual(config["services"]["nginx"]["ports"][0]["host_ip"], "127.0.0.1")
            self.assertNotIn(variables["POSTGRES_PASSWORD"].encode(), files["qnap/backend.env"].splitlines()[0])
            if not shutil.which("docker"):
                return
            for name, data in files.items():
                dest = folder / name
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(data)
            # Container Station's working directory may differ from the extracted project.
            checked = subprocess.run(["docker", "compose", "-f", str(folder / "qnap-compose.yaml"),
                                      "config", "--format", "json"], cwd=tmp, capture_output=True, check=True)
            options = json.loads(checked.stdout)["services"]["backend"]["environment"]
            # Compose doubles literal dollars in its serialised config export.
            self.assertEqual(options["JWT_SECRET"].replace("$$", "$"), secret)
            self.assertEqual(options["DATABASE_URL"], "db://user:test-db-password@postgres")
            self.assertEqual(options["PREFIX"].replace("$$", "$"), "music/$USD")
            self.assertEqual(options["EMPTY"], "")
