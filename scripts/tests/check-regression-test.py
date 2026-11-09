#!/usr/bin/env python3
"""Exercise the report and exit status using Criterion-shaped filesystem inputs."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check_regression.sh"


class RegressionCheckTests(unittest.TestCase):
    def run_check(self, cases):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for case, runs in cases.items():
                for run, value in runs.items():
                    path = root / "target/criterion" / case / run / "estimates.json"
                    path.parent.mkdir(parents=True)
                    path.write_text(json.dumps({"mean": {"point_estimate": value}}))
            process = subprocess.run(["sh", str(SCRIPT)], cwd=root, capture_output=True, text=True)
            report = (root / "benchmark-report.md").read_text()
            self.assertEqual(process.stdout, report)
            return process.returncode, report

    def test_all_depths_and_quoted_names(self):
        status, report = self.run_check({
            "simple": {"baseline": 100, "current": 105},
            "group/size/100": {"baseline": 100, "current": 80},
            "quote's name": {"baseline": 100, "current": 100},
        })
        self.assertEqual(status, 0)
        self.assertIn("3 benchmarks compared", report)

    def test_regression(self):
        status, report = self.run_check({"case": {"baseline": 100, "current": 106}})
        self.assertEqual(status, 1)
        self.assertIn("REGRESSION", report)

    def test_missing_or_invalid_measurements_fail(self):
        cases = [{}, {"case": {"current": 1}}, {"case": {"baseline": 1}}]
        cases += [{"case": {"baseline": value, "current": 1}}
                  for value in [0, -1, True, "1", None, float("nan"), float("inf")]]
        for case in cases:
            with self.subTest(case=case):
                status, report = self.run_check(case)
                self.assertEqual(status, 1)
                self.assertIn("ERROR", report)


if __name__ == "__main__":
    unittest.main()
