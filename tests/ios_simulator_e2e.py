"""Test installed iPhone/iPad runtimes using an owned temporary device set.

No runtime or guest app installation. Native Settings navigation only.
The simulator devices and screenshots are deleted even after assertion failures.
Run from the repository root after `cargo build -p cli`.
"""
import json
from pathlib import Path
import selectors
import subprocess
import tempfile
import time

SIMCTL = '/Library/Developer/PrivateFrameworks/CoreSimulator.framework/Versions/A/Resources/bin/simctl'

class Session:
    def __init__(self, device_set, udid):
        self.process = subprocess.Popen(
            ['target/debug/unimation', '--provider', 'ios', '--device-set', str(device_set), '--device', udid, 'session'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
    def call(self, **request):
        self.process.stdin.write(json.dumps(request) + '\n')
        self.process.stdin.flush()
        if not self.selector.select(45):
            raise TimeoutError('iOS session did not respond')
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError('iOS session exited unexpectedly')
        return json.loads(line)
    def result(self, **request):
        reply = self.call(**request)
        assert 'error' not in reply, reply
        return reply['result']
    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.selector.close()

def label(node):
    return node['attributes'].get('AXLabel', {}).get('value')

def observe_until(session, predicate):
    deadline = time.monotonic() + 20
    while True:
        snapshot = session.result(op='observe')
        if predicate(snapshot):
            return snapshot
        if time.monotonic() >= deadline:
            raise AssertionError('Expected Settings state not observed: ' + repr([label(n) for n in snapshot['nodes']]))
        time.sleep(0.2)

def exercise(device_set, udid, image):
    session = Session(device_set, udid)
    try:
        session.result(op='launch', bundle_id='com.apple.Preferences')
        initial = observe_until(session, lambda s: any(label(n) == 'General' for n in s['nodes']))
        again = session.result(op='observe')
        general = next(n for n in initial['nodes'] if label(n) == 'General')
        assert general['reference'] == next(n['reference'] for n in again['nodes'] if label(n) == 'General')
        text = session.result(op='view', format='text')
        assert 'General' in text and '@e' in text
        invalid = session.call(op='semantic', target={'session':'foreign','id':1}, action={'kind':'perform','name':'AXPress'})
        assert 'error' in invalid and invalid['error']['effect'] == 'none'
        receipt = session.result(op='semantic', target=general['reference'], action={'kind':'perform','name':'AXPress'})
        assert receipt['effect'] == 'dispatched'
        opened = observe_until(session, lambda s: any(label(n) == 'About' for n in s['nodes']))
        delta = session.result(op='diff', before=initial['revision'], after=opened['revision'])
        assert delta['before_revision'] == initial['revision']
        assert delta['after_revision'] == opened['revision']
        about = next(n for n in opened['nodes'] if label(n) == 'About')
        session.result(op='semantic', target=about['reference'], action={'kind':'perform','name':'AXPress'})
        details = observe_until(session, lambda s: any('Version' in (label(n) or '') or 'Model Name' in (label(n) or '') for n in s['nodes']))
        assert details['traversal_complete']
        changed = session.result(op='diff', before=opened['revision'], after=details['revision'])
        assert changed['newly_observed'] or changed['modified'], 'About navigation must change the observation'
        session.result(op='capture', path=str(image))
        assert image.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        print('PASS native Settings General/About navigation, retained reference, compact view, diff, foreign reference rejection, screenshot', flush=True)
    finally:
        session.close()

if __name__ == '__main__':
    runtime = next(r for r in json.loads(subprocess.check_output([SIMCTL,'list','runtimes','--json']))['runtimes'] if r.get('isAvailable') and r.get('platform') == 'iOS')
    with tempfile.TemporaryDirectory(prefix='unimation-ios-e2e-') as directory:
        device_set = Path(directory)
        for family in ['iPhone','iPad']:
            device_type = next(t for t in runtime['supportedDeviceTypes'] if t['productFamily'] == family)
            command = [SIMCTL, '--set', directory]
            udid = subprocess.check_output(command + ['create', 'Unimation disposable ' + family, device_type['identifier'], runtime['identifier']], text=True).strip()
            try:
                subprocess.run(command + ['bootstatus', udid, '-b'], check=True, timeout=180)
                print('Testing', family, udid, flush=True)
                exercise(device_set, udid, device_set / (family + '.png'))
            finally:
                subprocess.run(command + ['shutdown', udid], timeout=30)
                subprocess.run(command + ['delete', udid], check=True, timeout=30)
