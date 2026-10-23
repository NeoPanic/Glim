#!/usr/bin/env python3
"""Export sanitized event timelines, action/shortcut reach and resource summaries."""
import argparse
import csv
import json
import runpy
from collections import defaultdict
from pathlib import Path

helpers = runpy.run_path(str(Path(__file__).with_name('telemetry-format-report.py')))
records = helpers['records']
percentile = helpers['percentile']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('export', type=Path)
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    groups = defaultdict(lambda: {'events': 0, 'people': set(), 'sessions': set(), 'duration': []})
    shortcuts = defaultdict(lambda: {'events': 0, 'people': set()})
    resources = defaultdict(lambda: defaultdict(list))
    seen = set()
    with (args.output_dir / 'timeline.csv').open('w', newline='', encoding='utf-8-sig') as output:
        timeline = csv.writer(output)
        timeline.writerow(['timestamp', 'installation_id', 'session_id', 'event_sequence', 'event', 'format', 'reader', 'action', 'source', 'result', 'duration_ms', 'os_version', 'app_version', 'properties_json'])
        for event in records(args.export):
            props = event.get('props', event.get('properties', {})) or {}
            if isinstance(props, str):
                props = json.loads(props)
            system = event.get('systemProps', event.get('system_props', {})) or {}
            if isinstance(system, str):
                system = json.loads(system)
            name = event.get('eventName', event.get('event_name', event.get('name', 'unknown')))
            session = str(event.get('sessionId', event.get('session_id', '')))
            identity = props.get('installation_id', '')
            sequence = props.get('event_sequence')
            if sequence is not None and session:
                key = (identity, session, str(sequence))
                if key in seen:
                    continue
                seen.add(key)
            extension = props.get('file_extension', '')
            reader = props.get('reader_kind', '')
            action = props.get('action', '')
            source = props.get('input_source', '')
            result = props.get('result', '')
            timeline.writerow([event.get('timestamp', ''), identity, session, sequence, name, extension, reader, action, source, result, props.get('duration_ms', ''), system.get('osVersion', ''), system.get('appVersion', ''), json.dumps(props, ensure_ascii=False, separators=(',', ':'))])
            row = groups[(name, extension, reader, action, source, result)]
            row['events'] += 1
            if identity:
                row['people'].add(identity)
            if session:
                row['sessions'].add(session)
            duration = props.get('duration_ms')
            if isinstance(duration, (int, float)) and duration >= 0:
                row['duration'].append(duration)
            if name in {'shortcut_used', 'native_shortcut_used'}:
                modifiers = '+'.join(key for key in ['command', 'control', 'option', 'shift'] if props.get(key))
                key = props.get('key_token', 'keycode:' + str(props.get('key_code', 'unknown')))
                shortcut = shortcuts[(name, extension, reader, action, modifiers, key)]
                shortcut['events'] += 1
                if identity:
                    shortcut['people'].add(identity)
            if name == 'resource_sample':
                for metric in ['resident_bytes', 'physical_footprint_bytes', 'compressed_bytes', 'metal_allocated_bytes', 'cpu_percent_x100', 'open_tabs']:
                    value = props.get(metric)
                    if isinstance(value, (int, float)):
                        resources[(extension, reader, props.get('scope', 'unknown'))][metric].append(value)
    with (args.output_dir / 'actions.csv').open('w', newline='', encoding='utf-8-sig') as output:
        writer = csv.writer(output)
        writer.writerow(['event', 'format', 'reader', 'action', 'source', 'result', 'events', 'unique_installations', 'sessions', 'duration_p50_ms', 'duration_p95_ms'])
        for key, value in sorted(groups.items(), key=lambda item: (-len(item[1]['people']), -item[1]['events'], item[0])):
            writer.writerow([*key, value['events'], len(value['people']), len(value['sessions']), percentile(value['duration'], .5), percentile(value['duration'], .95)])
    with (args.output_dir / 'shortcuts.csv').open('w', newline='', encoding='utf-8-sig') as output:
        writer = csv.writer(output)
        writer.writerow(['event', 'format', 'reader', 'action', 'modifiers', 'key', 'uses', 'unique_installations'])
        for key, value in sorted(shortcuts.items(), key=lambda item: (-item[1]['events'], item[0])):
            writer.writerow([*key, value['events'], len(value['people'])])
    with (args.output_dir / 'resources.csv').open('w', newline='', encoding='utf-8-sig') as output:
        writer = csv.writer(output)
        writer.writerow(['active_format', 'active_reader', 'scope', 'metric', 'samples', 'p50', 'p95', 'max'])
        for key, metrics in sorted(resources.items()):
            for metric, values in sorted(metrics.items()):
                writer.writerow([*key, metric, len(values), percentile(values, .5), percentile(values, .95), max(values)])
    print('Created timeline.csv, actions.csv, shortcuts.csv, resources.csv in', args.output_dir)


if __name__ == '__main__':
    main()
