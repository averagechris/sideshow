#!/usr/bin/env python3
"""Reproduce the Sideshow WebP/AVIF quality, size, and timing evaluation."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path


FIXTURES = (
    {
        "name": "photo",
        "filename": "Fronalpstock_big.jpg",
        "url": "https://upload.wikimedia.org/wikipedia/commons/3/3f/Fronalpstock_big.jpg",
        "sha256": "24eb29eccdf0af691b406d1a3d22c0ef5d761cc454d8a917848c33958f6fc857",
        "license": "CC BY-SA 3.0; Hannes Röst",
        "source_page": "https://commons.wikimedia.org/wiki/File:Fronalpstock_big.jpg",
    },
    {
        "name": "screenshot",
        "filename": "Screenshot_of_ilo_sona_Like.png",
        "url": "https://upload.wikimedia.org/wikipedia/commons/a/ac/Screenshot_of_ilo_sona_Like.png",
        "sha256": "f23d15195c45da37c8e7a1e4fb5e97fc7759b41cb71c5260358f2fcb54f41fc0",
        "license": "CC0 1.0; kala pona Tonyu",
        "source_page": "https://commons.wikimedia.org/wiki/File:Screenshot_of_ilo_sona_Like.png",
    },
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iterations", type=int, default=3)
    parser.add_argument("--max-dim", type=int, default=3840)
    parser.add_argument("--speed", type=int, default=4)
    parser.add_argument(
        "--qualities", type=int, nargs="+", default=[40, 50, 60, 70, 80, 90, 95]
    )
    parser.add_argument("--output", type=Path, help="also write the JSON result here")
    parser.add_argument(
        "--workdir", type=Path, help="retain downloads and encoded outputs here"
    )
    args = parser.parse_args()
    if args.iterations < 1 or args.max_dim < 1:
        parser.error("iterations and max-dim must be greater than zero")
    if not 1 <= args.speed <= 10:
        parser.error("speed must be in 1..10")
    if any(not 1 <= quality <= 100 for quality in args.qualities):
        parser.error("qualities must be in 1..100")
    if 80 not in args.qualities:
        parser.error("qualities must include 80 (the current Sideshow WebP default)")
    return args


def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, **kwargs)


def download(fixture: dict[str, str], destination: Path) -> None:
    if not destination.exists():
        request = urllib.request.Request(
            fixture["url"], headers={"User-Agent": "sideshow-avif-evaluation/1"}
        )
        with urllib.request.urlopen(request) as response, destination.open("wb") as out:
            shutil.copyfileobj(response, out)
    digest = hashlib.sha256(destination.read_bytes()).hexdigest()
    if digest != fixture["sha256"]:
        destination.unlink(missing_ok=True)
        raise RuntimeError(
            f"checksum mismatch for {fixture['name']}: {digest} != {fixture['sha256']}"
        )


def encode(
    encoder: str,
    format_name: str,
    source: Path,
    output: Path,
    quality: int,
    speed: int,
    iterations: int,
    max_dim: int,
) -> dict[str, object]:
    process = run(
        [
            encoder,
            format_name,
            str(source),
            str(output),
            str(quality),
            str(speed),
            str(iterations),
            str(max_dim),
        ],
        capture_output=True,
    )
    return json.loads(process.stdout)


def ssim(reference: Path, encoded: Path) -> float:
    process = run(
        [
            "ffmpeg",
            "-hide_banner",
            "-i",
            str(reference),
            "-i",
            str(encoded),
            "-lavfi",
            "ssim",
            "-f",
            "null",
            "-",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
    )
    matches = re.findall(r"All:([0-9.]+)", process.stderr)
    if not matches:
        raise RuntimeError(f"ffmpeg did not report SSIM for {encoded}")
    return float(matches[-1])


def data_uri_size(byte_count: int, mime: str) -> int:
    return len(f"data:{mime};base64,") + 4 * math.ceil(byte_count / 3)


def evaluate(args: argparse.Namespace, workdir: Path) -> dict[str, object]:
    encoder = shutil.which("sideshow-avif-evaluation-encoder")
    if encoder is None:
        raise RuntimeError(
            "sideshow-avif-evaluation-encoder is not on PATH; use `nix run .#avif-evaluation`"
        )
    records: list[dict[str, object]] = []
    fixture_metadata: list[dict[str, object]] = []
    for fixture in FIXTURES:
        source = workdir / fixture["filename"]
        download(fixture, source)
        reference = workdir / f"{fixture['name']}-reference.png"
        reference_info = encode(
            encoder, "reference", source, reference, 80, args.speed, 1, args.max_dim
        )
        source_bytes = source.stat().st_size
        fixture_metadata.append(
            {
                **fixture,
                "source_bytes": source_bytes,
                "evaluated_width": reference_info["width"],
                "evaluated_height": reference_info["height"],
            }
        )
        for format_name in ("webp", "avif"):
            for quality in args.qualities:
                output = workdir / f"{fixture['name']}-q{quality}.{format_name}"
                record = encode(
                    encoder,
                    format_name,
                    source,
                    output,
                    quality,
                    args.speed,
                    args.iterations,
                    args.max_dim,
                )
                record.update(
                    {
                        "fixture": fixture["name"],
                        "source_bytes": source_bytes,
                        "data_uri_bytes": data_uri_size(
                            int(record["output_bytes"]), f"image/{format_name}"
                        ),
                        "ssim": ssim(reference, output),
                        "only_if_smaller": int(record["output_bytes"]) < source_bytes,
                        "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                    }
                )
                records.append(record)

    comparisons = []
    for fixture in FIXTURES:
        fixture_name = fixture["name"]
        webp = next(
            record
            for record in records
            if record["fixture"] == fixture_name
            and record["format"] == "webp"
            and record["quality"] == 80
        )
        candidates = [
            record
            for record in records
            if record["fixture"] == fixture_name
            and record["format"] == "avif"
            and float(record["ssim"]) >= float(webp["ssim"])
        ]
        avif = min(candidates, key=lambda record: int(record["output_bytes"])) if candidates else None
        comparisons.append(
            {
                "fixture": fixture_name,
                "equivalence_rule": "AVIF SSIM >= current WebP quality-80 SSIM",
                "webp": webp,
                "avif": avif,
                "output_size_change_percent": (
                    round(
                        (int(avif["output_bytes"]) / int(webp["output_bytes"]) - 1)
                        * 100,
                        2,
                    )
                    if avif
                    else None
                ),
                "encode_time_multiple": (
                    round(
                        float(avif["encode_median_ms"])
                        / float(webp["encode_median_ms"]),
                        2,
                    )
                    if avif
                    else None
                ),
            }
        )

    return {
        "schema": 1,
        "environment": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "encoder": "Cargo.lock-pinned image 0.25.10 + webp 0.3.1; see package metadata",
            "ffmpeg": run(["ffmpeg", "-version"], capture_output=True)
            .stdout.splitlines()[0],
        },
        "settings": {
            "qualities": args.qualities,
            "avif_speed": args.speed,
            "threads": 1,
            "iterations": args.iterations,
            "max_dim": args.max_dim,
            "timing": "median encode-only; one untimed warmup; decode/resize/write excluded",
            "quality_metric": "FFmpeg SSIM All against lossless resized PNG reference",
        },
        "fixtures": fixture_metadata,
        "records": records,
        "equivalent_quality_comparisons": comparisons,
    }


def main() -> int:
    args = parse_args()
    temporary = None
    if args.workdir:
        workdir = args.workdir.resolve()
        workdir.mkdir(parents=True, exist_ok=True)
    else:
        temporary = tempfile.TemporaryDirectory(prefix="sideshow-avif-")
        workdir = Path(temporary.name)
    result = evaluate(args, workdir)
    rendered = json.dumps(result, indent=2, sort_keys=True)
    print(rendered)
    if args.output:
        args.output.write_text(rendered + "\n")
    if args.workdir:
        print(f"artifacts: {workdir}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
