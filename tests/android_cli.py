"""Android CLI checks that require neither authorization nor a connected phone."""
import json
import os
import subprocess

env={k:v for k,v in os.environ.items() if not k.startswith('ACTUATE_')}
def run(*args):
    return subprocess.run(['target/debug/actuate',*args],text=True,capture_output=True,env=env)
r=run('--provider','android','capabilities','--json')
assert r.returncode==0,r
caps=json.loads(r.stdout)
assert not caps['adb_executable_required'] and not caps['adb_server_required']
assert not caps['observation'] and not caps['hid']
for args,message in [
    (('--provider','android','capture','/tmp/unused','--display','1'),'alternate capture'),
    (('--provider','android','session'),'requires --credentials'),
    (('--provider','android','capabilities','--device-set','/tmp'),'CoreSimulator'),
]:
    r=run(*args)
    assert r.returncode and message in r.stderr,r
r=run('android','pair-qr','--help')
assert r.returncode==0 and '--qr-svg' in r.stdout and '--credentials' in r.stdout,r
print('Android CLI routing checks passed')
