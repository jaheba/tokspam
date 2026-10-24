import hashlib
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from tools import case_db


class ResourceCampaignImportTest(unittest.TestCase):
    def make_campaign(self, root):
        source = b"x = 1\n"
        (root / "case.py").write_bytes(source)
        (root / "resource-summary.json").write_text(json.dumps({"format": "spam-resource-campaign-v1"}))
        record = {
            "case_id": hashlib.sha256(source).hexdigest(),
            "source": "case.py",
            "family": "test",
            "depth": 1,
            "generator": {"name": "test", "parameters": {"count": 1}},
            "labels": ["resource-control"],
            "observations": [{
                "implementation": "cpython-test",
                "phase": "parse",
                "outcome": "within-limit",
                "status": "ok",
                "samples_seconds": [0.001, 0.002, 0.003],
                "median_seconds": 0.002,
                "time_limit_seconds": 0.1,
            }],
        }
        (root / "resource-results.jsonl").write_text(json.dumps(record) + "\n")
        return record

    def test_imports_recipe_labels_and_samples(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = self.make_campaign(root)
            with case_db.connect(root / "cases.sqlite") as database:
                case_db.import_resource_campaign(database, root)
                case_id = record["case_id"]
                self.assertEqual(
                    database.execute(
                        "SELECT generator, parameters FROM case_generators WHERE case_id = ?", (case_id,)
                    ).fetchone(),
                    ("test", '{"count": 1}'),
                )
                self.assertEqual(
                    database.execute(
                        "SELECT outcome, sample_count, median_seconds FROM resource_observations WHERE case_id = ?",
                        (case_id,),
                    ).fetchone(),
                    ("within-limit", 3, 0.002),
                )
                self.assertEqual(
                    database.execute(
                        "SELECT phase, implementation_version FROM observation_context WHERE case_id = ?",
                        (case_id,),
                    ).fetchone(),
                    ("parse", None),
                )

    def test_rejects_source_digest_mismatch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.make_campaign(root)
            records = [json.loads(line) for line in (root / "resource-results.jsonl").read_text().splitlines()]
            records[0]["case_id"] = "0" * 64
            (root / "resource-results.jsonl").write_text(json.dumps(records[0]) + "\n")
            with case_db.connect(root / "cases.sqlite") as database:
                with self.assertRaisesRegex(ValueError, "digest"):
                    case_db.import_resource_campaign(database, root)

    def test_regenerates_source_from_recipe(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            record = self.make_campaign(root)
            (root / "generate.py").write_text(
                "def generate(name, parameters):\n"
                "    assert name == 'test'\n"
                "    return 'x = ' + str(parameters['count']) + '\\n'\n"
            )
            (root / "resource-summary.json").write_text(
                json.dumps({"format": "spam-resource-campaign-v1", "generator": "generate.py"})
            )
            record.pop("source")
            (root / "resource-results.jsonl").write_text(json.dumps(record) + "\n")
            with case_db.connect(root / "cases.sqlite") as database:
                case_db.import_resource_campaign(database, root)
                self.assertEqual(
                    database.execute("SELECT source FROM cases WHERE id = ?", (record["case_id"],)).fetchone(),
                    (b"x = 1\n",),
                )

    def test_parser_campaign_records_phase_and_versions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = b"x = 1\n"
            digest = hashlib.sha256(source).hexdigest()
            (root / "case.py").write_bytes(source)
            (root / "summary.json").write_text(json.dumps({
                "phase": "compile",
                "python_version": "Python test",
                "ruff_probe_sha256": "ruff-digest",
                "spam_probe_sha256": "spam-digest",
            }))
            record = {
                "case_id": digest,
                "artifact": "case.py",
                "family": "test",
                "depth": 1,
                "seed": 1,
                "cpython": {"status": "MemoryError"},
                "ruff": {"status": "ok"},
                "spam": {"status": "reject:Nesting:0:1"},
            }
            (root / "results.jsonl").write_text(json.dumps(record) + "\n")

            with case_db.connect(root / "cases.sqlite") as database:
                case_db.import_campaign(database, root)
                contexts = database.execute(
                    "SELECT implementation, phase, implementation_version FROM observation_context "
                    "ORDER BY implementation"
                ).fetchall()
                self.assertEqual(contexts, [
                    ("cpython", "compile", "Python test"),
                    ("ruff", "compile", "ruff-digest"),
                    ("spam", "screen", "spam-digest"),
                ])


if __name__ == "__main__":
    unittest.main()
