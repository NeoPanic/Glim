#!/usr/bin/env python3
"""Rank format reach from a JSON/JSONL/CSV event export. No API credentials needed."""
import argparse
import csv
import json
import math
from collections import defaultdict
from pathlib import Path


def records(path):
    if path.suffix.lower() == '.csv':
        with path.open(newline='', encoding='utf-8-sig') as stream:
            yield from csv.DictReader(stream)
        return
    source = path.read_text(encoding='utf-8-sig')
    try:
        data = json.loads(source)
    except json.JSONDecodeError:
        for line in source.splitlines():
            if line.strip():
                yield json.loads(line)
        return
    if isinstance(data, dict):
        data = data.get('events', data.get('data', [data]))
    if not isinstance(data, list):
        raise ValueError('Expected event array or {events: [...]} / {data: [...]}')
    yield from data


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else ''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('export', type=Path)
    parser.add_argument('--output', type=Path, help='CSV output; defaults to stdout')
    args = parser.parse_args()
    formats = defaultdict(lambda: dict(opens=0, failed=0, people=set(), sessions=set(), latency=[], native_failed=0, dwell_ms=0, unidentified=0))
    all_people = set()
    seen = set()
    for event in records(args.export):
        props = event.get('props', event.get('properties', {})) or {}
        if isinstance(props, str):
            props = json.loads(props)
        name = event.get('eventName', event.get('event_name', event.get('name')))
        session = str(event.get('sessionId', event.get('session_id', '')))
        identity = props.get('installation_id')
        if identity:
            all_people.add(identity)
        # Re-imported identical export rows must not inflate usage. Older health
        # events and repeat aggregates need not have a sequence and are ignored.
        sequence = props.get('event_sequence')
        if sequence is not None and session:
            key = (identity, session, sequence)
            if key in seen:
                continue
            seen.add(key)
        extension = props.get('file_extension')
        if not extension:
            continue
        row = formats[extension]
        if name == 'file_open':
            row['opens'] += 1
            row['failed'] += props.get('result') == 'failed'
            if identity:
                row['people'].add(identity)
            else:
                row['unidentified'] += 1
            if session:
                row['sessions'].add(session)
            duration = props.get('duration_ms')
            if isinstance(duration, (float, int)) and duration >= 0:
                row['latency'].append(duration)
        elif name == 'native_load' and props.get('result') == 'failed':
            row['native_failed'] += 1
        elif name == 'foreground_segment':
            row['dwell_ms'] += props.get('duration_ms', 0)
    import sys
    output = args.output.open('w', newline='', encoding='utf-8-sig') if args.output else sys.stdout
    try:
        writer = csv.writer(output)
        writer.writerow(['format', 'unique_installations', 'reach_percent', 'open_attempts', 'sessions', 'open_failed', 'open_failure_percent', 'open_p50_ms', 'open_p95_ms', 'native_load_failed', 'foreground_seconds', 'opens_without_identity'])
        for extension, row in sorted(formats.items(), key=lambda item: (-len(item[1]['people']), -item[1]['opens'], item[0])):
            if not row['opens']:
                continue
            writer.writerow([extension, len(row['people']), round(100 * len(row['people']) / len(all_people), 2) if all_people else '', row['opens'], len(row['sessions']), row['failed'], round(100 * row['failed'] / row['opens'], 2), percentile(row['latency'], .5), percentile(row['latency'], .95), row['native_failed'], round(row['dwell_ms'] / 1000, 1), row['unidentified']])
    finally:
        if args.output:
            output.close()


if __name__ == '__main__':
    main()
