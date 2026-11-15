#!/usr/bin/env python3
"""Replay pinned assessment cases without exposing labels or prior evidence."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
MANIFEST = HERE / 'cases.json'
EXCLUDED = {'evidence', 'audits', 'benchmarks', '.git'}


def command(args, **kwargs):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, **kwargs)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def materialize(case, root):
    archive = command(['git', 'archive', case['revision']], cwd=REPO).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        for item in source:
            path = Path(item.name)
            if path.parts[0] in EXCLUDED:
                continue
            if path.is_absolute() or '..' in path.parts or not (item.isfile() or item.isdir()):
                raise ValueError('Unsupported archive entry: ' + item.name)
            target = root / path
            if item.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(source.extractfile(item).read())
                target.chmod(item.mode)
    for edit in case['edits']:
        path = root / edit['path']
        text = path.read_text()
        if text.count(edit['old']) != edit['count']:
            raise ValueError('Mutation anchor changed: ' + edit['path'])
        path.write_text(text.replace(edit['old'], edit['new']))
    # No commit history, evidence, labels, or audit rationales in model-visible tree.
    command(['git', 'init', '--quiet'], cwd=root)
    command(['git', 'add', '-A'], cwd=root)
    digest = hashlib.sha256()
    for path in sorted(root.rglob('*')):
        if path.is_file() and '.git' not in path.relative_to(root).parts:
            digest.update(str(path.relative_to(root)).encode() + b'\0')
            digest.update(path.read_bytes() + b'\0')
    actual = digest.hexdigest()
    if actual != case["tree_sha256"]:
        raise ValueError("Snapshot digest changed: " + case["id"])
    return actual


def classify(expected, evidence, returncode):
    if returncode != 0 or not isinstance(evidence, dict):
        return 'execution_failure'
    actual = evidence.get('determination')
    if type(actual) is not bool:
        return 'unexpected_judgment'
    if actual == expected:
        return 'label_match_pending_rationale_review'
    return 'false_accept' if actual else 'false_alarm'


def run_case(case, args, destination):
    destination.mkdir()
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='habeas-bench-') as temp:
        temp = Path(temp)
        root = temp / 'project'
        root.mkdir()
        tree_hash = materialize(case, root)
        # Isolate usage history while retaining the user's named model profiles.
        state = temp / 'state'
        env = dict(os.environ, XDG_STATE_HOME=str(state))
        invocation = [str(args.binary), 'author', case['subject'], '--stdout',
                      '--policy', 'once', '--config', args.config]
        timed_out = False
        try:
            completed = subprocess.run(invocation, cwd=root, env=env,
                                       capture_output=True, timeout=args.timeout)
            stdout, stderr, returncode = completed.stdout, completed.stderr, completed.returncode
        except subprocess.TimeoutExpired as error:
            stdout, stderr, returncode = error.stdout or b'', error.stderr or b'', None
            timed_out = True
        (destination / 'stdout.txt').write_bytes(stdout)
        (destination / 'stderr.txt').write_bytes(stderr)
        try:
            evidence = json.loads(stdout)
        except (ValueError, UnicodeError):
            evidence = None
        usage = []
        for path in sorted(state.glob('habeas/authoring/usage/*.json')):
            usage.append(json.loads(path.read_text()))
        write_json(destination / 'usage.json', usage)
        if evidence is not None:
            write_json(destination / 'evidence.json', evidence)
        result = dict(case=case['id'], pair=case['pair'], subject=case['subject'],
                      revision=case['revision'], tree_sha256=tree_hash,
                      expected=case['expected_determination'],
                      actual=evidence.get('determination') if isinstance(evidence, dict) else None,
                      classification=classify(case['expected_determination'], evidence, returncode),
                      label_status=case['label_status'], returncode=returncode,
                      timed_out=timed_out, elapsed_seconds=round(time.monotonic()-start, 3),
                      rationale_review='pending', profile=args.config)
        write_json(destination / 'result.json', result)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['check', 'run'])
    parser.add_argument('--case', action='append', dest='cases', help='Repeat to select cases')
    parser.add_argument('--config', help='Existing named authoring profile (required for run)')
    parser.add_argument('--binary', type=Path, default=REPO / 'target/debug/habeas')
    parser.add_argument('--output', type=Path, help='New results directory outside the checkout')
    parser.add_argument('--repetitions', type=int, default=1)
    parser.add_argument('--timeout', type=float, default=600)
    args = parser.parse_args()
    manifest_bytes = MANIFEST.read_bytes()
    cases = json.loads(manifest_bytes)['cases']
    if args.cases:
        unknown = set(args.cases) - {case['id'] for case in cases}
        if unknown:
            parser.error('Unknown cases: ' + ', '.join(sorted(unknown)))
        cases = [case for case in cases if case['id'] in args.cases]
    if args.repetitions < 1 or args.timeout <= 0:
        parser.error('Repetitions and timeout must be positive')
    if args.action == 'check':
        for case in cases:
            with tempfile.TemporaryDirectory(prefix='habeas-bench-check-') as root:
                digest = materialize(case, Path(root))
                print(case['id'], digest)
        return
    if not args.config or not args.output:
        parser.error('run requires --config and --output; run sends model requests')
    args.binary = args.binary.resolve(strict=True)
    args.output = args.output.resolve()
    if args.output == REPO or REPO in args.output.parents:
        parser.error('Output must be outside the checkout to avoid leaking benchmark answers')
    args.output.mkdir(parents=True, exist_ok=False)
    write_json(args.output / 'run.json', dict(
        manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
        binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        harness_revision=command(['git', 'rev-parse', 'HEAD'], cwd=REPO).stdout.decode().strip(),
        harness_dirty=bool(command(['git', 'status', '--porcelain'], cwd=REPO).stdout),
        profile=args.config, policy='once', repetitions=args.repetitions,
        note='Resolved backend/model and usage are recorded per case; profile options must be held fixed externally.'))
    results = []
    for repetition in range(args.repetitions):
        for case in cases:
            result = run_case(case, args, args.output / (case['id'] + '-' + str(repetition + 1)))
            results.append(result)
            write_json(args.output / 'results.json', results)
            print(result['case'], result['classification'], flush=True)
            # Do not fan out or repeatedly consume a known exhausted quota.
            usage = json.loads((args.output / (case['id'] + '-' + str(repetition + 1)) / 'usage.json').read_text())
            codes = {request.get('failure', {}).get('error_code')
                     for record in usage for request in record.get('requests', [])}
            if codes & {'credit_balance_exhausted', 'insufficient_quota',
                        'organization_spend_limit_exceeded', 'project_spend_limit_exceeded',
                        'organization_usage_limit_exceeded'}:
                print('Stopping after a billing/quota error; partial results saved.')
                return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
