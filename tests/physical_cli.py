"""Physical iOS routing contracts; no device connection or pairing required."""
import json
import os
import subprocess

base = {k: v for k, v in os.environ.items() if not k.startswith('ACTUATE_')}
def run(*args):
    return subprocess.run(['target/debug/actuate', '--provider', 'apple-device', *args], env=base, text=True, capture_output=True)
r=run('capabilities','--json')
assert r.returncode == 0, r
caps=json.loads(r.stdout)
assert caps['discovery'] and not caps['input'] and not caps['observation']
assert caps['capture_availability']=='unprobed' and not caps['streaming']
for args, message in [
    (('capture','/tmp/actuate-should-not-exist.bin'),'requires --device'),
    (('snapshot',),'currently supports'),
    (('session',),'currently supports'),
    (('discover','--scope','apps'),'currently supports'),
    (('capabilities','--device-set','/tmp'),'CoreSimulator'),
    (('capture','/tmp/actuate-should-not-exist.bin','--display','1'),'unavailable'),
]:
    r=run(*args)
    assert r.returncode and message in r.stderr, r
print('Physical CLI routing contracts passed')

for group in ('simulators', 'devices'):
    r = run('apple', group, '--help')
    assert r.returncode == 0 and 'list' in r.stdout, r
r = run('--help')
assert 'apple-device' in r.stdout and 'apple-simulator' in r.stdout, r
