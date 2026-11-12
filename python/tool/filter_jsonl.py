# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
filter_jsonl.py

Usage:
    filter_jsonl.py [<n>] --input=<input_path> --output=<output_path> [--random] [--force]
    filter_jsonl.py (-h | --help)
    filter_jsonl.py --version

Arguments:
    n                         Number of unique VINs to filter. Default is 1.

Options:
    -i --input=<input_path>      Path to input JSONL file. Required.
    -o --output=<output_path>    Path to output JSONL file. Required.
    -r --random                  Select VINs randomly instead of first appearance. Default is first appearance.
    -f --force                   Overwrite output file if it already exists.
    -h --help                 Show this help message.
    --version                 Show version.

Description:
    Filters the input JSONL file by selecting n unique VINs either by first appearance (default)
    or randomly, and writes all lines whose VIN is among the selected VINs to another JSONL file.

Notes:
    - Each line in the input JSONL file is expected to be a valid JSON object containing a "vin" field.
    - Lines without a valid JSON object or without a "vin" field are skipped.
"""

import json
import os
import random
import sys
from collections.abc import Iterable
from typing import Any

from docopt import docopt

VERSION = "1.0.0"


def read_jsonl_lines(input_file_path: str) -> Iterable[str]:
    with open(input_file_path, encoding="utf-8") as file:
        yield from file


def parse_json_safe(json_text: str) -> dict[str, Any] | None:
    try:
        return json.loads(json_text)
    except json.JSONDecodeError as error:
        print(f"Warning: Skipping invalid JSON line: {error}", file=sys.stderr)
        return None


def collect_unique_vins_in_order_from_path(input_file_path: str) -> list[str]:
    seen_vins: set[str] = set()
    ordered_vins: list[str] = []
    for line in read_jsonl_lines(input_file_path):
        json_object = parse_json_safe(line)
        if not isinstance(json_object, dict):
            continue
        for vin in json_object:
            if vin not in seen_vins:
                seen_vins.add(vin)
                ordered_vins.append(vin)
    return ordered_vins


def select_vins_first(ordered_vins: list[str], number_of_vins: int) -> set[str]:
    return set(ordered_vins[:number_of_vins])


def select_vins_random(ordered_vins: list[str], number_of_vins: int) -> set[str]:
    if number_of_vins >= len(ordered_vins):
        return set(ordered_vins)
    # random.sample requires a sequence
    return set(random.sample(ordered_vins, number_of_vins))


def filter_lines_by_vins_stream(
    input_file_path: str, selected_vins: set[str], output_file_path: str
) -> int:
    written = 0
    with open(output_file_path, "w", encoding="utf-8") as outfile:
        for line in read_jsonl_lines(input_file_path):
            json_object = parse_json_safe(line)
            if not isinstance(json_object, dict):
                continue
            record_list: dict[str, Any] = {}
            for vin, value in json_object.items():
                if vin in selected_vins:
                    record_list[vin] = value
            if record_list:
                outfile.write(json.dumps(record_list, ensure_ascii=False) + "\n")
                written += 1
    return written


def main() -> None:
    try:
        arguments = docopt(__doc__, version=VERSION)
    except SystemExit:
        # docopt already prints Usage on parsing errors; re-raise to exit
        raise

    input_file_path: str = arguments.get("--input")
    output_file_path: str = arguments.get("--output")
    force_overwrite: bool = bool(arguments.get("--force"))

    # Extra safety: if somehow missing, show usage-like error
    if not input_file_path or not output_file_path:
        print("Error: --input and --output are required options.\n", file=sys.stderr)
        print(__doc__.split("Description:")[0].strip(), file=sys.stderr)
        sys.exit(1)

    # Prevent accidental overwrite unless --force is provided
    if os.path.exists(output_file_path) and not force_overwrite:
        print(
            f"Error: Output file already exists: {output_file_path}. Use --force to overwrite.",
            file=sys.stderr,
        )
        sys.exit(1)

    n_argument = arguments.get("<n>")
    try:
        number_of_vins: int = int(n_argument) if n_argument is not None else 1
    except ValueError:
        print("Error: n must be an integer.", file=sys.stderr)
        sys.exit(1)
    if number_of_vins <= 0:
        print("Error: n must be a positive integer.", file=sys.stderr)
        sys.exit(1)

    try:
        # First pass: collect all unique VINs by streaming the input
        ordered_vins: list[str] = collect_unique_vins_in_order_from_path(
            input_file_path
        )
    except FileNotFoundError:
        print(f"Error: Input file not found: {input_file_path}", file=sys.stderr)
        sys.exit(1)
    except OSError as error:
        print(
            f"Error: Failed to read input file '{input_file_path}': {error}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"Found {len(ordered_vins)} unique VIN(s) in input.")
    if not ordered_vins:
        print(
            "Warning: No valid VINs found in input. Output will be empty.",
            file=sys.stderr,
        )
        # Create/overwrite empty output file
        try:
            with open(output_file_path, "w", encoding="utf-8") as _:
                pass
        except OSError as error:
            print(
                f"Error: Failed to write output file '{output_file_path}': {error}",
                file=sys.stderr,
            )
            sys.exit(1)
        sys.exit(0)

    number_of_vins = min(number_of_vins, len(ordered_vins))

    use_random_selection: bool = bool(arguments.get("--random"))
    if use_random_selection:
        selected_vins: set[str] = select_vins_random(ordered_vins, number_of_vins)
    else:
        selected_vins = select_vins_first(ordered_vins, number_of_vins)

    try:
        written_count = filter_lines_by_vins_stream(
            input_file_path, selected_vins, output_file_path
        )
    except OSError as error:
        print(
            f"Error: Failed to write output file '{output_file_path}': {error}",
            file=sys.stderr,
        )
        sys.exit(1)

    print(
        f"Filtered {len(selected_vins)} VIN(s). Wrote {written_count} line(s) to {output_file_path}."
    )


if __name__ == "__main__":
    main()
