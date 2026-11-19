import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from scenario import ScenarioError, observe_zero_cache


class ReaderVolumeTests(unittest.TestCase):
    def test_detects_inline_and_partial_audio_while_allowing_local_metadata(self):
        pcm = bytes(i * 7 % 127 + 64 for i in range(128))
        reader = Mock()
        reader.call.return_value = {"cache": dict.fromkeys(
            ("maximum_bytes", "reserved_bytes", "pinned_bytes", "evictable_bytes"), 0)}
        with tempfile.TemporaryDirectory() as tmp, patch.dict(os.environ, {
                "SOUNDTIME_TEST_COMPOSE_PROJECT": "", "SOUNDTIME_TEST_FAULT_DRIVER": "fixture",
                "SOUNDTIME_TEST_READER_VOLUME_B": tmp}):
            root = Path(tmp)
            store = root / "p2p-blobs"
            (store / "cache-v2").mkdir(parents=True)
            (store / "data").mkdir()
            (store / "cache-v2" / ".owner").touch()
            (root / "music").mkdir()
            (root / "music" / "local.wav").write_bytes(b"local original")
            # A key with the pattern must never be opened by the payload scan.
            (root / "secret_key").write_bytes(pcm)
            db = store / "blobs.db"
            db.write_bytes(b"\0catalogue metadata")
            snapshots = {}
            observe_zero_cache(reader, [pcm], snapshots, "before_remote_play")
            db.write_bytes(b"\0updated catalogue metadata")
            (store / "data" / "local.obao4").write_bytes(b"\0proof")
            observe_zero_cache(reader, [pcm], snapshots, "metadata_only")
            self.assertIn("p2p-blobs/data/local.obao4", snapshots["metadata_only"]["proof_files"])

            db.write_bytes(b"\0catalogue" + pcm + b"\0")
            with self.assertRaises(ScenarioError):
                observe_zero_cache(reader, [pcm], snapshots, "inline_audio")
            self.assertEqual(snapshots["inline_audio"]["remote_pcm_files"], ["p2p-blobs/blobs.db"])
            db.write_bytes(b"\0catalogue")
            partial = store / "cache-v2" / ".part-abcd"
            partial.mkdir()
            with self.assertRaises(ScenarioError):
                observe_zero_cache(reader, [pcm], snapshots, "empty_stage")
            self.assertIn("p2p-blobs/cache-v2/.part-abcd", snapshots["empty_stage"]["partial_paths"])
            partial.rmdir()
            (root / "music" / "unexpected").write_bytes(b"x")
            with self.assertRaises(ScenarioError):
                observe_zero_cache(reader, [pcm], snapshots, "unclassified_file")
