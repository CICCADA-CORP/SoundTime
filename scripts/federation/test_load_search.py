import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import load_search


class LocalSearchPlans(unittest.TestCase):
    def test_waits_for_the_initial_job_even_if_cursor_is_null(self):
        state = {}
        with patch.object(load_search, "sql", side_effect=[
            '{"ready":false,"last_track_id":null}', '{"ready":true,"last_track_id":null}'
        ]), patch.object(load_search.time, "sleep"):
            load_search.wait_for_index(None, "fixture", state, 20)
        self.assertTrue(state["ready"])
        self.assertEqual(state["polls"], 2)

    def test_plans_use_application_sql_and_keep_going_after_timeout(self):
        calls = []

        def run(project, *args, data, timeout):
            script = data.decode()
            self.assertIn("BEGIN READ ONLY", script)
            self.assertIn("statement_timeout = '8s'", script)
            self.assertLessEqual(timeout, 14)
            names = ("tracks", "artists", "albums", "name_matches")
            category, = [n for n in names if (load_search.QUERIES / (n + ".sql")).read_text() in script]
            calls.append((category, args[-1], "force_generic_plan" in script))
            if category == "artists":
                raise subprocess.CalledProcessError(3, ["psql"], stderr=b"statement timeout")
            return b'[{"Execution Time":1,"Planning Time":2,"Plan":{"Actual Rows":10}}]'

        with tempfile.TemporaryDirectory() as folder:
            result = load_search.explain(run, "fixture", Path(folder) / "report.json")
            plans = json.loads((Path(folder) / result["plan_file"]).read_text())
        self.assertEqual(len(plans), 8)
        self.assertEqual(len(result["samples"]), 8)
        for term in ["term=Quartz0001:*", "term=Melody00:*"]:
            self.assertIn(("tracks", term, True), calls)
            self.assertIn(("tracks", term, False), calls)
            self.assertIn(("name_matches", term, True), calls)
        self.assertEqual(next(p for p in plans if p["category"] == "artists")["error"], "statement timeout")
        self.assertIn("explain", plans[-1])
