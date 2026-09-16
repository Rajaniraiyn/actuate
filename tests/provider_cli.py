"""Provider parsing and completion contracts, without touching native UI."""
import os
import subprocess
import sys

BIN = 'target/debug/unimation'
base = {k: v for k, v in os.environ.items() if not k.startswith('UNIMATION_')}

def run(*args, **env):
    return subprocess.run([BIN, *args], env=base | env, text=True, capture_output=True)

if sys.platform == 'darwin':
    r = run('session', UNIMATION_PROVIDER='apple-simulator')
    assert r.returncode and 'requires --device' in r.stderr, r
    r = run('--provider', 'apple-simulator', 'session', UNIMATION_PROVIDER='invalid')
    assert r.returncode and 'requires --device' in r.stderr, r
    r = run('session', '--provider', 'apple-simulator', UNIMATION_DEVICE='test')
    assert r.returncode and 'requires --device-set' in r.stderr, r
else:
    r = run('session', UNIMATION_PROVIDER='apple-simulator')
    assert r.returncode and 'invalid value' in r.stderr, r
for args, env in [(('session',), {'UNIMATION_PROVIDER': 'invalid'}),
                  (('snapshot', '--format', 'invalid'), {}),
                  (('snapshot', '--scope', 'invalid'), {}),
                  (('capture', '/tmp/unused.png', '--backend', 'invalid'), {})]:
    r = run(*args, **env)
    assert r.returncode and 'invalid' in r.stderr, r
for shell in ('bash', 'zsh', 'fish'):
    r = run('completions', shell)
    assert r.returncode == 0 and 'unimation' in r.stdout, r
r = run('spec')
assert r.returncode == 0 and 'UNIMATION_PROVIDER' in r.stdout
assert 'ios-session' not in r.stdout and 'ios-list' not in r.stdout
if sys.platform == 'darwin':
    r = run('apple', 'simulators', 'list', '--help')
    assert r.returncode == 0 and '--device-set' in r.stdout

r = run('view', '/nonexistent', '--json', '--format', 'text')
assert r.returncode and 'cannot be used with' in r.stderr.lower(), r
for extra, expect_json in [((), False), (('--json',), True)]:
    r = subprocess.run([BIN, 'session', *extra], env=base, input='not json\n', text=True, capture_output=True)
    assert r.returncode == 0, r
    assert r.stdout.startswith('{') == expect_json, r
    assert 'invalid_request' in r.stdout, r

print('Provider CLI contracts passed')
