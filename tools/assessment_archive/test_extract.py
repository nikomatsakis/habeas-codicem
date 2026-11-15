import json
from pathlib import Path
import tempfile
import unittest

from extract import extract, save


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.usage = self.root / 'usage'
        self.usage.mkdir()
        for run in ['a', 'b']:
            (self.usage / f'{run}.json').write_text(json.dumps({
                'run_id': run, 'subject': 'requirements.foo', 'determination': 'false',
                'start_time_ms': 1, 'tokens': {'input': 10},
            }))

    def log(self, body):
        path = self.root / 'run.log'
        path.write_text(body)
        return path

    def test_batch_preserves_history_and_excludes_footer(self):
        log = self.log('Authoring 2 assessments\nrequirements.foo: false (usage: /old/a.json)\nFirst gap.\n\n'
                       'requirements.foo: false (usage: /old/b.json)\nSecond gap.\n2 authored; 0 skipped\nCoverage warnings\nwarning\n')
        records = extract([self.usage], [log])
        self.assertEqual(records['a']['rationale'], 'First gap.')
        self.assertEqual(records['b']['rationale'], 'Second gap.')
        output = self.root / 'archive.jsonl'
        save(records, output)
        original = output.read_bytes()
        save(extract([self.usage], [log]), output)
        self.assertEqual(output.read_bytes(), original)
        self.assertEqual(len(output.read_text().splitlines()), 2)

    def test_single_and_missing_rationale(self):
        log = self.log('requirements.foo: false\nGap.\nrequirements.foo: openai / sol; policy once; 0 skipped\nEvidence: /evidence.json\nUsage: /old/a.json\n')
        records = extract([self.usage], [log])
        self.assertEqual(records['a']['rationale'], 'Gap.')
        self.assertIsNone(records['b']['rationale'])
        self.assertEqual(records['b']['rationale_status'], 'not_in_supplied_logs')

    def test_mismatch_rejected(self):
        log = self.log('requirements.other: false (usage: /old/a.json)\nGap.\n')
        with self.assertRaisesRegex(ValueError, 'mismatch'):
            extract([self.usage], [log])

    def test_missing_usage_rejected(self):
        log = self.log('requirements.foo: false (usage: /old/unknown.json)\nGap.\n')
        with self.assertRaisesRegex(ValueError, 'no supplied usage'):
            extract([self.usage], [log])

    def test_existing_rationale_survives_metadata_only_import(self):
        log = self.log('requirements.foo: false (usage: /old/a.json)\nGap.\n')
        output = self.root / 'archive.jsonl'
        save(extract([self.usage], [log]), output)
        records = save(extract([self.usage], []), output)
        self.assertEqual(records[0]['rationale'], 'Gap.')

    def test_passing_result_has_no_invented_rationale(self):
        path = self.usage / 'a.json'
        usage = json.loads(path.read_text())
        usage['determination'] = 'true'
        path.write_text(json.dumps(usage))
        log = self.log('requirements.foo: true (usage: /old/a.json)\n1 authored; 0 skipped\n')
        self.assertIsNone(extract([self.usage], [log])['a']['rationale'])

    def test_conflicting_import_preserves_archive(self):
        log = self.log('requirements.foo: false (usage: /old/a.json)\nGap.\n')
        output = self.root / 'archive.jsonl'
        save(extract([self.usage], [log]), output)
        original = output.read_bytes()
        changed = self.log('requirements.foo: false (usage: /old/a.json)\nDifferent gap.\n')
        with self.assertRaisesRegex(ValueError, 'archive conflicts'):
            save(extract([self.usage], [changed]), output)
        self.assertEqual(output.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
