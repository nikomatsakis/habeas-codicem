"""Offline checks for benchmark isolation, reconstruction, and result accounting."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('assessment_runner', Path(__file__).with_name('run.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class BenchmarkTests(unittest.TestCase):
    def test_all_cases_are_reproducible_and_answers_are_not_in_snapshots(self):
        cases = json.loads(runner.MANIFEST.read_text())['cases']
        self.assertEqual(len({case['id'] for case in cases}), len(cases))
        pairs = {}
        for case in cases:
            pairs.setdefault(case['pair'], []).append(case['expected_determination'])
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                runner.materialize(case, root)
                for excluded in runner.EXCLUDED - {'.git'}:
                    self.assertFalse((root / excluded).exists())
                self.assertTrue((root / 'Cargo.toml').exists())
                history = runner.subprocess.run(['git', 'rev-parse', '--verify', 'HEAD'],
                                                cwd=root, capture_output=True)
                self.assertNotEqual(history.returncode, 0)
                for edit in case['edits']:
                    self.assertNotIn(edit['old'], (root / edit['path']).read_text())
        self.assertTrue(all(sorted(labels) == [False, True] for labels in pairs.values()))

    def test_malformed_or_failed_attempt_is_not_a_wrong_judgment(self):
        self.assertEqual(runner.classify(True, None, 0), 'execution_failure')
        self.assertEqual(runner.classify(True, {'determination': False}, 1), 'execution_failure')
        self.assertEqual(runner.classify(True, {'determination': 'FIXME'}, 0), 'unexpected_judgment')
        self.assertEqual(runner.classify(False, {'determination': True}, 0), 'false_accept')
        self.assertEqual(runner.classify(True, {'determination': False}, 0), 'false_alarm')

    def test_run_captures_usage_without_a_provider(self):
        case = json.loads(runner.MANIFEST.read_text())['cases'][0]
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            fake = temp / 'fake-habeas'
            fake.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
assert '--stdout' in sys.argv and sys.argv[sys.argv.index('--policy')+1] == 'once'
assert not pathlib.Path('evidence').exists()
p = pathlib.Path(os.environ['XDG_STATE_HOME']) / 'habeas/authoring/usage'
p.mkdir(parents=True)
(p / 'test.json').write_text(json.dumps({'backend':'fake', 'tokens':{'input':17}}))
print(json.dumps({'determination':False, 'rationale':'offline fixture'}))
''')
            fake.chmod(0o755)
            args = SimpleNamespace(binary=fake, config='fake', timeout=30)
            output = temp / 'result'
            result = runner.run_case(case, args, output)
            self.assertEqual(result['classification'], 'label_match_pending_rationale_review')
            self.assertEqual(json.loads((output / 'usage.json').read_text())[0]['tokens']['input'], 17)
            self.assertEqual(result['rationale_review'], 'pending')


if __name__ == '__main__':
    unittest.main()
