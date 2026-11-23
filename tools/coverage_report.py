#!/usr/bin/env python3
"""Render the Rust coverage export and require instrumented branch data."""
import argparse
import json
from pathlib import Path

METRICS = ("lines", "branches", "functions", "regions")
PROVIDERS = (
    "src/live/proxmox.rs",
    "src/live/proxmox/cluster.rs",
    "src/live/proxmox/backup.rs",
    "src/live/kube.rs",
    "src/live/cloud.rs",
    "src/live/cloud/aws.rs",
    "src/live/agents.rs",
)


def read_report(path):
    data, = json.loads(path.read_text())["data"]
    return data


def display(stat):
    return f"{stat['percent']:.2f}% ({stat['covered']:,}/{stat['count']:,})"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--min-lines", type=float, default=0)
    parser.add_argument("--min-branches", type=float, default=0)
    args = parser.parse_args()

    report = read_report(args.report)
    totals = report["totals"]
    if not totals.get("branches", {}).get("count"):
        parser.error("report has no instrumented branch data")
    baseline = read_report(args.baseline)["totals"] if args.baseline else None

    heading = "| Metric | Baseline | Current | Change |" if baseline else "| Metric | Coverage |"
    print("# Rust coverage\n")
    print(heading)
    print("|---|---|---|---|" if baseline else "|---|---|")
    for metric in METRICS:
        stat = totals[metric]
        if baseline:
            before = baseline[metric]
            change = stat["percent"] - before["percent"]
            print(f"| {metric.title()} | {display(before)} | {display(stat)} | {change:+.2f} pp |")
        else:
            print(f"| {metric.title()} | {display(stat)} |")

    print("\n| Provider source | Lines | Branches |")
    print("|---|---|---|")
    for provider in PROVIDERS:
        summaries = [item["summary"] for item in report["files"] if item["filename"].endswith("/" + provider)]
        if len(summaries) != 1:
            parser.error(f"expected one summary for {provider}")
        summary = summaries[0]
        print(f"| `{provider}` | {display(summary['lines'])} | {display(summary['branches'])} |")

    failures = []
    for metric, threshold in (("lines", args.min_lines), ("branches", args.min_branches)):
        if totals[metric]["percent"] < threshold:
            failures.append(f"{metric} coverage is below {threshold:.2f}%")
    if failures:
        parser.error(", ".join(failures))


if __name__ == "__main__":
    main()
