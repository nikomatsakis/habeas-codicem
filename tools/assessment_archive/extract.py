#!/usr/bin/env python3
"""Archive saved Habeas usage metadata and verbose CLI rationales, without inference."""
import argparse
import hashlib
import json
from pathlib import Path
import re

HEADER = re.compile(r'^(\S+): (true|false|FIXME)(?: \(usage: (.+)\))?$')
STOP = re.compile(r'^(?:\d+ authored;|Coverage warnings|Evidence:|Usage:|\S+: .+; policy )')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def log_entries(path):
    raw = path.read_bytes()
    lines = raw.decode('utf-8').splitlines()
    entries = []
    i = 0
    while i < len(lines):
        match = HEADER.fullmatch(lines[i])
        if not match:
            i += 1
            continue
        subject, determination, usage_path = match.groups()
        start = i
        i += 1
        while i < len(lines) and not HEADER.fullmatch(lines[i]) and not STOP.match(lines[i]):
            i += 1
        rationale = '\n'.join(lines[start + 1:i]).strip() or None
        # Single-subject CLI reports its usage file after the rationale/footer.
        if usage_path is None:
            footer_end = next((j for j in range(i, len(lines)) if HEADER.fullmatch(lines[j])), len(lines))
            paths = [line.removeprefix('Usage: ').strip() for line in lines[i:footer_end] if line.startswith('Usage: ')]
            if len(paths) != 1:
                raise ValueError(f'{path}:{start + 1}: cannot identify one usage record')
            usage_path = paths[0]
        entries.append({
            'run_id': Path(usage_path).stem, 'subject': subject,
            'determination': determination, 'rationale': rationale,
            'source': {'path': str(path.resolve()), 'sha256': digest(raw),
                       'line_start': start + 1, 'line_end': max(start + 1, i)},
        })
    return entries


def extract(usage_dirs, logs):
    records = {}
    for directory in usage_dirs:
        paths = sorted(directory.rglob('*.json'))
        for path in paths:
            raw = path.read_bytes()
            usage = json.loads(raw)
            if 'run_id' not in usage or 'subject' not in usage:
                raise ValueError(f'{path}: not a usage record; supply a usage directory')
            run_id = usage['run_id']
            source = {'path': str(path.resolve()), 'sha256': digest(raw)}
            if run_id in records:
                if records[run_id]['usage'] != usage:
                    raise ValueError(f'conflicting usage for {run_id}')
                if source not in records[run_id]['usage_sources']:
                    records[run_id]['usage_sources'].append(source)
                continue
            records[run_id] = {
                'schema_version': 1, 'run_id': run_id, 'subject': usage['subject'],
                'usage': usage, 'usage_sources': [source],
                'rationale': None, 'rationale_status': 'not_in_supplied_logs',
                'rationale_sources': [], 'rationale_sha256': None,
            }
    for log in logs:
        for entry in log_entries(log):
            run_id = entry['run_id']
            if run_id not in records:
                raise ValueError(f'{log}: no supplied usage record for {run_id}')
            record = records[run_id]
            if entry['subject'] != record['subject'] or entry['determination'] != record['usage'].get('determination'):
                raise ValueError(f'{log}: subject/outcome mismatch for {run_id}')
            if not entry['rationale']:
                continue
            if record['rationale'] not in (None, entry['rationale']):
                raise ValueError(f'{log}: conflicting rationales for {run_id}')
            record['rationale'] = entry['rationale']
            record['rationale_status'] = 'recovered_from_cli_log'
            record['rationale_sha256'] = digest(entry['rationale'].encode())
            if entry['source'] not in record['rationale_sources']:
                record['rationale_sources'].append(entry['source'])
    return records


def save(records, output):
    # Merge by run ID: rerunning extraction is idempotent; assessments never overwrite one another.
    previous = {}
    if output.exists():
        previous = {r['run_id']: r for r in map(json.loads, output.read_text().splitlines())}
    for run_id, record in records.items():
        if run_id in previous:
            old = previous[run_id]
            if old['usage'] != record['usage'] or (old['rationale'] and record['rationale'] and old['rationale'] != record['rationale']):
                raise ValueError(f'archive conflicts with extraction for {run_id}')
            for key in ['usage_sources', 'rationale_sources']:
                record[key] = old[key] + [x for x in record[key] if x not in old[key]]
            if record['rationale'] is None:
                for key in ['rationale', 'rationale_status', 'rationale_sha256']:
                    record[key] = old[key]
        previous[run_id] = record
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + '.tmp')
    ordered = sorted(previous.values(), key=lambda r: (r['usage'].get('start_time_ms', 0), r['run_id']))
    temporary.write_text(''.join(json.dumps(r, sort_keys=True, ensure_ascii=False) + '\n' for r in ordered))
    temporary.replace(output)
    return ordered


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--usage-dir', action='append', type=Path, required=True)
    parser.add_argument('--log', action='append', type=Path, default=[])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    for directory in args.usage_dir:
        if not directory.is_dir() or not any(directory.rglob('*.json')):
            parser.error(f'usage directory missing or empty: {directory}')
    records = save(extract(args.usage_dir, args.log), args.output)
    print(json.dumps({'records': len(records), 'subjects': len({r['subject'] for r in records}),
                      'rationales': sum(r['rationale'] is not None for r in records),
                      'output': str(args.output)}))


if __name__ == '__main__':
    main()
