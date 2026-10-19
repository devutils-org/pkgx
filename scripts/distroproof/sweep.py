#!/usr/bin/env python3
"""Guest-only checks over the installed archives. Compatible with Python 3.6+."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
from collections import Counter


def execute(args, timeout=120, env=None):
    result = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            universal_newlines=True, timeout=timeout, env=env)
    return {"args": [str(a) for a in args], "code": result.returncode,
            "stdout": result.stdout, "stderr": result.stderr}


def success(result):
    if result['code'] != 0:
        raise ValueError('command failed: ' + result['stderr'][-2000:])
    return result


def document(result):
    success(result)
    value = json.loads(result['stdout'])
    if value.get('schema_version') != 1:
        raise ValueError('missing or unknown xpac JSON schema')
    return value


def native_inventory(provider):
    """Compare every name, architecture and version, retaining repeated RPM kegs."""
    if provider == 'apt':
        raw = success(execute(['dpkg-query', '-W', '-f=${Package}\t${Architecture}\t${Version}\t${db:Status-Status}\n']))['stdout']
        return Counter(tuple(row.split('\t')[:3]) for row in raw.splitlines()
                       if row.split('\t')[-1] == 'installed')
    if provider == 'pacman':
        # Different native interface from xpac's -Qi paragraph parser.
        raw = success(execute(['pacman', '-Q']))['stdout']
        arch = success(execute(['pacman-conf', 'Architecture']))['stdout'].strip()
        # Per-package architecture can also be "any", so query the local db.
        values = []
        for desc in Path('/var/lib/pacman/local').glob('*/desc'):
            fields = desc.read_text().split('\n\n')
            record = dict((f.splitlines()[0], f.splitlines()[1:]) for f in fields if f.strip())
            values.append((record['%NAME%'][0], record['%ARCH%'][0], record['%VERSION%'][0]))
        if Counter((n, v) for n, a, v in values) != Counter(tuple(r.split()) for r in raw.splitlines()):
            raise ValueError('pacman native list and local database disagree (' + arch + ')')
        return Counter(values)
    if provider == 'dnf':
        raw = success(execute(['rpm', '-qa', '--qf', '%{NAME}\t%{ARCH}\t%{EPOCHNUM}:%{VERSION}-%{RELEASE}\n']))['stdout']
        return Counter(tuple(row.split('\t')) for row in raw.splitlines()
                       if row.split('\t')[0] != 'gpg-pubkey')
    raise ValueError('no native inventory witness for ' + provider)


def sweep(flavor, info, cell):
    binary = '/opt/xpac/' + flavor + '/xpac'
    build = {"flavor": flavor, "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
             "checks": [], "providers": []}
    cell['builds'].append(build)
    if build['binary_sha256'] != info['binary_sha256']:
        raise ValueError('installed binary differs from the packaged build')
    if flavor == 'gnu' and Path('/lib/ld-musl-x86_64.so.1').exists():
        build['status'] = 'not_applicable'
        build['reason'] = 'GNU archive needs a glibc loader; use the static musl archive on Alpine'
        return

    def check(name, action):
        entry = {'name': name, 'status': 'pass'}
        build['checks'].append(entry)
        try:
            detail = action()
            if detail is not None:
                entry['detail'] = detail
        except Exception as error:
            entry['status'] = 'fail'
            entry['error'] = str(error)
            print(flavor + ': FAIL ' + name + ': ' + str(error), flush=True)
        return entry.get('detail')

    def cli(*args):
        return execute([binary] + list(args))

    check('version', lambda: success(cli('--version')))
    providers = check('providers', lambda: document(cli('providers', '--json')))
    check('doctor', lambda: document(cli('doctor', '--json')))
    check('demo', lambda: document(cli('demo', '--snapshot', '--json')))
    for shell in ('bash', 'zsh', 'fish'):
        check('completion-' + shell, lambda shell=shell: success(cli('completions', shell)))
    with tempfile.TemporaryDirectory(prefix='xpac-cell-') as tmp:
        state = str(Path(tmp) / 'state')
        check('missing-history', lambda: document(cli('history', '--state-dir', state, '--json')))
        check('inspection-no-state', lambda: assert_true(not Path(state).exists(), 'inspection created state'))
        # The existing file checks write only within this fresh private HOME.
        env = dict(os.environ, XPAC_DISPOSABLE='1', XPAC_BIN=binary,
                   XPAC_ROOT=str(Path(tmp) / 'files'), PATH='/usr/local/bin:' + os.environ.get('PATH', ''))
        if os.getuid() == 0:
            def root_refusal():
                source = Path(tmp) / 'root-file'
                source.write_text('root must not adopt this file')
                result = execute([binary, 'file', 'adopt', str(source), '--yes', '--state-dir', str(Path(tmp) / 'root-state'), '--json'], env=dict(os.environ, HOME=tmp))
                error = json.loads(result['stderr'].strip().splitlines()[-1])['error']
                assert_true(result['code'] != 0 and 'never as root' in error, 'managed file write was not refused as root')
                return result
            check('managed-files', root_refusal)
        else:
            check('managed-files', lambda: success(execute(['sh', 'tests/sandbox/files-checks.sh'], env=env)))
        for provider in (providers or {}).get('providers', []):
            name = provider['provider']
            supported = provider['available'] and provider['compatibility']['minimum_met'] is True
            build['providers'].append({'provider': name, 'available': provider['available'],
                                       'supported': supported, 'version': provider['version'],
                                       'diagnostic': provider['diagnostic']})
            if not supported:
                def refuse(name=name):
                    result = cli('list', '--provider', name, '--json')
                    value = json.loads(result['stderr'])
                    assert_true(result['code'] != 0 and value.get('schema_version') == 1 and value.get('error'),
                                'unavailable/old provider was not explicitly refused')
                    return result
                check('refusal-' + name, refuse)
                continue
            inventory = check('inventory-' + name, lambda name=name: document(cli('list', '--provider', name, '--json')))
            if name in ('apt', 'pacman', 'dnf'):
                def compare(name=name, inventory=inventory):
                    expected = native_inventory(name)
                    observed = Counter((p['id']['name'], p['id']['architecture_or_scope'], p['version'])
                                       for p in (inventory or {})['packages'])
                    assert_true(observed == expected,
                                'inventory mismatch: missing=' + repr(list((expected-observed).items())[:10]) +
                                ', extra=' + repr(list((observed-expected).items())[:10]))
                    return {'packages': sum(expected.values())}
                check('native-inventory-' + name, compare)
                for command in ('info', 'why', 'deps'):
                    check(command + '-' + name, lambda command=command, name=name: document(cli(command, name + ':bash', '--json')))
                check('search-' + name, lambda name=name: document(cli('search', 'bash', '--installed', '--provider', name, '--json')))
                manifest = str(Path(tmp) / (name + '.toml'))
                check('export-' + name, lambda name=name, manifest=manifest: success(cli('export', '--provider', name, '-o', manifest)))
                def roundtrip(manifest=manifest):
                    diff = document(cli('diff', '-f', manifest, '--json'))['data']
                    assert_true(not diff['missing'], 'export is missing installed identities')
                    plan = document(cli('plan', '-f', manifest, '--json'))['data']
                    assert_true(all(not p['plan']['operations'] for p in plan['providers']), 'export planned package changes')
                    return {'missing': len(diff['missing']), 'providers': len(plan['providers'])}
                check('roundtrip-' + name, roundtrip)
    build['status'] = 'fail' if any(c['status'] == 'fail' for c in build['checks']) else 'pass'
    print(flavor + ': ' + build['status'] + ', ' + str(len(build['checks'])) + ' checks', flush=True)


def assert_true(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    target = os.environ.get('DP_TARGET', '')
    if (Path.cwd() != Path('/distroproof/work') or not target or
            Path('/etc/xpac-distroproof-guest').read_text().strip() != target):
        raise SystemExit('this sweep requires a prepared distroproof guest')
    cell = {'schema_version': 1, 'target': target, 'image': os.environ['DP_IMAGE'],
            'kernel': platform.release(), 'uid': os.getuid(), 'builds': [], 'transactions': []}
    builds = json.loads(Path('dist/distroproof/builds.json').read_text())
    try:
        for flavor in ('gnu', 'musl'):
            try:
                sweep(flavor, builds[flavor], cell)
            except Exception as error:
                cell.setdefault('errors', []).append(flavor + ': ' + str(error))
        if target.startswith('transactions-'):
            assert_true(os.getuid() == 0, 'transaction matrix must run as guest root')
            image = os.environ['DP_IMAGE']
            scripts = (['transactions.sh', 'privilege.sh'] if image.startswith(('debian-', 'ubuntu-')) else
                       ['pacman-transactions.sh'] if image == 'arch' else ['dnf-transactions.sh'])
            for script in scripts:
                result = execute(['sh', 'tests/containers/' + script], timeout=480)
                result['name'] = script
                cell['transactions'].append(result)
                print('transaction ' + script + ': ' + str(result['code']), flush=True)
    except Exception as error:
        cell.setdefault('errors', []).append(str(error))
    finally:
        Path('cell.json').write_text(json.dumps(cell, indent=2) + '\n')
    failed = (cell.get('errors') or any(b['status'] == 'fail' for b in cell['builds']) or
              any(t['code'] != 0 for t in cell['transactions']))
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
