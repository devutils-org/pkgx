#!/usr/bin/env python3
"""Judge collected cells against the configured matrix and the current archives.

An empty/partial matrix, missing check, failed setup, stale binary or disagreement
with envprobe is a failure, even if distroproof's command exited successfully.
"""
import argparse
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
PROVIDERS = {'apt', 'pacman', 'dnf', 'flatpak', 'brew'}
BASE = {'version', 'providers', 'doctor', 'demo', 'completion-bash',
        'completion-zsh', 'completion-fish', 'missing-history',
        'inspection-no-state', 'managed-files'}


def fact(report, section, key):
    for group in report.get('sections', []):
        if group['name'] == section:
            for entry in group['entries']:
                if entry['key'] == key and entry['status'] == 'ok':
                    return entry['value']
    raise ValueError('envprobe missing ' + section + '.' + key)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def judge(target, cell, expected, builds):
    name = expected['name']
    require(target['name'] == name and target['image'] == expected['image'], 'target identity mismatch')
    require(target['outcome'] == 'success', 'VM outcome: ' + target['outcome'] + ': ' +
            ((target.get('command') or {}).get('stderr') or (target.get('setup') or {}).get('stderr') or target.get('error') or '')[-1200:])
    require((target.get('setup') or {}).get('exit_code') == 0, 'guest setup failed')
    require((target.get('command') or {}).get('exit_code') == 0, 'guest command failed')
    require(cell['schema_version'] == 1 and cell['target'] == name and cell['image'] == expected['image'], 'cell identity/schema mismatch')
    require(not cell.get('errors'), 'cell errors: ' + repr(cell.get('errors')))
    probe = target.get('envprobe') or {}
    require(cell['kernel'] == fact(probe, 'kernel', 'release'), 'kernel disagrees with envprobe')
    require(str(cell['uid']) == str(fact(probe, 'privileges', 'uid')), 'uid disagrees with envprobe')
    user = target['variant']['user']
    require(user == expected.get('_user', 'test'), 'test user differs from the configured matrix')
    require((cell['uid'] == 0) == (user == 'root'), 'wrong test identity')
    by_flavor = {b['flavor']: b for b in cell['builds']}
    require(set(by_flavor) == {'gnu', 'musl'} and len(cell['builds']) == 2, 'missing/duplicate build')
    managers = set(fact(probe, 'packages', 'managers').split(', '))
    summary = []
    for flavor, build in by_flavor.items():
        require(build['binary_sha256'] == builds[flavor]['binary_sha256'], flavor + ': stale binary')
        if build['status'] == 'not_applicable':
            require(flavor == 'gnu' and fact(probe, 'libc', 'kind') == 'musl', 'unexpected build exclusion')
            require(bool(build.get('reason')), 'build exclusion needs a reason')
            summary.append(flavor + ': use musl')
            continue
        require(build['status'] == 'pass', flavor + ': failed checks: ' + repr(
            [(c['name'], c.get('error')) for c in build['checks'] if c['status'] != 'pass']))
        checks = {c['name']: c for c in build['checks']}
        require(len(checks) == len(build['checks']), 'duplicate check')
        require(all(c['status'] == 'pass' for c in checks.values()), flavor + ': failed check hidden by build status')
        require(BASE <= set(checks), flavor + ': missing base checks ' + repr(BASE - set(checks)))
        providers = {p['provider']: p for p in build['providers']}
        require(set(providers) == PROVIDERS and len(build['providers']) == len(PROVIDERS), 'missing/duplicate provider diagnostics')
        advertised = checks['providers']['detail']['providers']
        require({p['provider'] for p in advertised} == PROVIDERS and len(advertised) == len(PROVIDERS), 'incomplete raw providers')
        for provider in advertised:
            name = provider['provider']
            supported = provider['available'] and provider['compatibility']['minimum_met'] is True
            if provider['available'] and name in {'apt', 'pacman', 'dnf'}:
                require(name in managers, 'provider presence disagrees with envprobe: ' + name)
            require(supported == providers[name]['supported'], 'provider support disagrees with diagnostics')
            require(not supported or 'inventory' in provider['capabilities'], 'supported adapter lacks inventory capability')
            required = {'inventory-' + name} if supported else {'refusal-' + name}
            if supported and name in {'apt', 'pacman', 'dnf'}:
                required |= {prefix + '-' + name for prefix in ('native-inventory', 'info', 'why', 'deps', 'search', 'export', 'roundtrip')}
                count = checks.get('native-inventory-' + name, {}).get('detail', {}).get('packages', 0)
                require(count > 0, 'native witness must include installed packages')
            require(required <= set(checks), flavor + ': missing provider checks ' + repr(required - set(checks)))
        available = [p['provider'] for p in build['providers'] if p['supported']]
        summary.append(flavor + ': ' + (', '.join(available) if available else 'no supported native provider'))
    transactions = cell['transactions']
    if expected['name'].startswith('transactions-'):
        expected_scripts = ({'transactions.sh', 'privilege.sh'} if expected['image'].startswith(('debian-', 'ubuntu-')) else
                            {'pacman-transactions.sh'} if expected['image'] == 'arch' else {'dnf-transactions.sh'})
        require({t['name'] for t in transactions} == expected_scripts and len(transactions) == len(expected_scripts), 'missing transaction checks')
        require(all(t['code'] == 0 for t in transactions), 'transaction check failed')
    else:
        require(not transactions, 'inspection cell must not run native transactions')
    return {'target': expected['name'], 'image': expected['image'], 'kernel': cell['kernel'],
            'uid': cell['uid'], 'builds': summary, 'transactions': len(transactions)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--matrix', type=Path, default=ROOT / 'distroproof.toml')
    parser.add_argument('--only', help='Explicit subset of comma-separated target names; never implied by a partial report')
    parser.add_argument('--builds', type=Path, default=ROOT / 'dist/distroproof/builds.json')
    parser.add_argument('--output', type=Path, default=ROOT / 'dist/matrix/cells')
    args = parser.parse_args()
    config = tomllib.loads(args.matrix.read_text())
    expected = {t['name']: dict(t, _user=t.get('as_user', config.get('run', {}).get('as_user', 'root')))
                for t in config['target']}
    if args.only:
        chosen = set(args.only.split(','))
        require(chosen <= set(expected), 'unknown --only target')
        expected = {name: expected[name] for name in chosen}
    report = json.loads(args.report.read_text())
    targets = report['targets']
    require({t['name'] for t in targets} == set(expected) and len(targets) == len(expected), 'report target set does not match the requested matrix')
    builds = json.loads(args.builds.read_text())
    failures, rows = [], []
    for target in targets:
        name = target['name']
        try:
            collected = [Path(p) for p in target.get('collected', []) if Path(p).name == 'cell.json']
            require(len(collected) == 1, 'exactly one collected cell.json is required')
            cell = json.loads(collected[0].read_text())
            row = judge(target, cell, expected[name], builds)
            rows.append(row)
            directory = args.output / name
            directory.mkdir(parents=True, exist_ok=True)
            (directory / 'cell.json').write_text(json.dumps(cell, indent=2) + '\n')
            (directory / 'envprobe.json').write_text(json.dumps(target['envprobe'], indent=2) + '\n')
            print(name + ': ' + ' | '.join(row['builds']))
        except (ValueError, KeyError, OSError, TypeError) as error:
            failures.append({'target': name, 'error': str(error)})
            print(name + ': FAIL: ' + str(error), file=sys.stderr)
    args.output.mkdir(parents=True, exist_ok=True)
    summary = {'report': str(args.report), 'matrix': str(args.matrix), 'rows': rows, 'failures': failures}
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return 1 if failures else 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, KeyError, OSError) as error:
        sys.exit(str(error))
