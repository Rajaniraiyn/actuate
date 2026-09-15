"""Provider parsing and completion contracts, without touching native UI."""
import os
import subprocess

BIN = 'target/debug/unimation'
base = {k: v for k, v in os.environ.items() if not k.startswith('UNIMATION_')}

def run(*args, **env):
    return subprocess.run([BIN, *args], env=base | env, text=True, capture_output=True)

r = run('session', UNIMATION_PROVIDER='ios')
assert r.returncode and 'requires --device' in r.stderr, r
r = run('--provider', 'ios', 'session', UNIMATION_PROVIDER='invalid')
assert r.returncode and 'requires --device' in r.stderr, r
r = run('session', '--provider', 'ios', UNIMATION_DEVICE='test')
assert r.returncode and 'requires --device-set' in r.stderr, r
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
r = run('ios', 'simulators', 'list', '--help')
assert r.returncode == 0 and '--device-set' in r.stdout
print('Provider CLI contracts passed')
