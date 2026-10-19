#!/usr/bin/env python3
"""Negative controls: a judge must reject fabricated success and missing evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest
import json
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location('matrix_check', Path(__file__).with_name('check.py'))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


def baseline():
    diagnostics = [{'provider': p, 'available': False, 'capabilities': [],
                    'compatibility': {'minimum_met': None}} for p in sorted(check.PROVIDERS)]
    checks = [{'name': name, 'status': 'pass'} for name in sorted(check.BASE)]
    next(c for c in checks if c['name'] == 'providers')['detail'] = {'providers': diagnostics}
    checks += [{'name': 'refusal-' + p, 'status': 'pass'} for p in sorted(check.PROVIDERS)]
    cell = {'schema_version': 1, 'target': 'alpine', 'image': 'alpine', 'kernel': '6.18', 'uid': 1000,
            'transactions': [], 'builds': [
                {'flavor': 'gnu', 'binary_sha256': 'gnu', 'status': 'not_applicable', 'reason': 'musl loader'},
                {'flavor': 'musl', 'binary_sha256': 'musl', 'status': 'pass', 'checks': checks,
                 'providers': [{'provider': p, 'supported': False} for p in sorted(check.PROVIDERS)]}]}
    probe = {'sections': [{'name': section, 'entries': [{'key': key, 'status': 'ok', 'value': value}]}
                          for section, key, value in [('kernel', 'release', '6.18'), ('privileges', 'uid', '1000'), ('libc', 'kind', 'musl'), ('packages', 'managers', 'apt, apk')]]}
    target = {'name': 'alpine', 'image': 'alpine', 'outcome': 'success', 'setup': {'exit_code': 0},
              'command': {'exit_code': 0}, 'envprobe': probe, 'variant': {'user': 'test'}}
    expected = {'name': 'alpine', 'image': 'alpine'}
    builds = {flavor: {'binary_sha256': flavor} for flavor in ('gnu', 'musl')}
    return target, cell, expected, builds


class JudgeTests(unittest.TestCase):
    def test_complete_evidence_passes(self):
        self.assertEqual(check.judge(*baseline())['target'], 'alpine')

    def test_rejects_missing_checks_failure_and_stale_binary(self):
        mutations = [
            lambda t, c: c['builds'][1]['checks'].pop(),
            lambda t, c: c['builds'][1]['checks'][0].update(status='fail'),
            lambda t, c: c['builds'][1].update(binary_sha256='old'),
            lambda t, c: t.update(outcome='error'),
            lambda t, c: t['setup'].update(exit_code=1),
            lambda t, c: c.update(kernel='another-kernel'),
            lambda t, c: c.update(uid=0),
            lambda t, c: c['builds'][0].update(status='pass', checks=[]),
            lambda t, c: c['builds'][1].update(providers=[]),
            lambda t, c: c.update(transactions=[{'name': 'surprise', 'code': 0}]),
        ]
        for mutate in mutations:
            with self.subTest(mutate=mutate):
                target, cell, expected, builds = copy.deepcopy(baseline())
                mutate(target, cell)
                with self.assertRaises((ValueError, KeyError)):
                    check.judge(target, cell, expected, builds)

    def test_empty_or_partial_report_is_not_a_matrix_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            matrix = root / 'matrix.toml'
            matrix.write_text('[[target]]\nname = "alpine"\nimage = "alpine"\n')
            report = root / 'report.json'
            report.write_text(json.dumps({'targets': []}))
            result = subprocess.run([sys.executable, str(Path(check.__file__)), str(report),
                                     '--matrix', str(matrix)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('target set', result.stderr)

    def test_supported_inventory_cannot_silently_skip_its_witness(self):
        target, cell, expected, builds = baseline()
        build = cell['builds'][1]
        provider = next(p for p in build['providers'] if p['provider'] == 'apt')
        provider['supported'] = True
        raw = next(c for c in build['checks'] if c['name'] == 'providers')['detail']['providers']
        apt = next(p for p in raw if p['provider'] == 'apt')
        apt.update(available=True, compatibility={'minimum_met': True}, capabilities=['inventory'])
        build['checks'].append({'name': 'inventory-apt', 'status': 'pass'})
        with self.assertRaisesRegex(ValueError, 'native witness'):
            check.judge(target, cell, expected, builds)


if __name__ == '__main__':
    unittest.main()
