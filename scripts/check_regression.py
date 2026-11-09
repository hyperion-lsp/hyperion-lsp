#!/usr/bin/env python3
"""Compare Criterion's named baseline/current estimates at any nesting depth."""

import json
import math
from pathlib import Path
import sys


def estimate(path):
    value = json.loads(path.read_text())["mean"]["point_estimate"]
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{path}: mean must be a number")
    if not math.isfinite(value) or value <= 0:
        raise ValueError(f"{path}: mean must be finite and positive")
    return value


def compare(root):
    # The union detects missing runs in either direction, including removed cases.
    cases = sorted({p.parent.parent for name in ("baseline", "current")
                    for p in root.rglob(f"{name}/estimates.json")})
    rows, errors = [], []
    regressions = 0
    if not cases:
        errors.append(f"No baseline/current estimates found in {root}")
    for case in cases:
        name = case.relative_to(root).as_posix()
        try:
            baseline = estimate(case / "baseline/estimates.json")
            current = estimate(case / "current/estimates.json")
            change = (current - baseline) / baseline * 100
            if not math.isfinite(change):
                raise ValueError(f"{name}: percentage change is not finite")
        except (OSError, ValueError, KeyError, TypeError, OverflowError) as error:
            errors.append(f"{name}: {error}")
            continue
        status = "REGRESSION" if change > 5 else "IMPROVEMENT" if change < -5 else "OK"
        regressions += status == "REGRESSION"
        rows.append(f"- {status} `{name}`: {baseline:g} ns → {current:g} ns ({change:+.2f}%)")
    failed = bool(errors or regressions)
    report = "\n".join([
        "### Performance Analysis", "",
        f"**Status:** {'FAILED' if failed else 'PASSED'}", "",
        *rows, *(f"- ERROR: {error}" for error in errors), "",
        f"**Summary:** {len(rows)} benchmarks compared; {regressions} regressions; "
        f"{len(errors)} errors. Threshold: ±5%.", "",
    ])
    return report, failed


def main():
    if len(sys.argv) != 1:
        print(f"Usage: {sys.argv[0]}", file=sys.stderr)
        return 2
    report, failed = compare(Path("target/criterion"))
    Path("benchmark-report.md").write_text(report)
    print(report, end="")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
