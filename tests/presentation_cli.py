"""Portable CLI presentation contract; no native UI or permissions required."""
import copy
import json
import subprocess
import tempfile
from pathlib import Path

BIN = 'target/debug/unimation'


def run(*args):
    return subprocess.check_output([BIN, *map(str, args)], text=True)


def main():
    def ref(i): return {'session': 'portable-test', 'id': i}
    def node(i, role, title, children=()):
        return {'reference': ref(i), 'attributes': {
            'AXRole': {'type': 'string', 'value': role},
            'AXTitle': {'type': 'string', 'value': title}},
            'children': [ref(c) for c in children], 'actions': ['AXPress'] if i != 1 else [],
            'parameterized_attributes': [], 'issues': []}
    before = {'root': ref(1), 'revision': 1, 'complete': True, 'traversal_complete': True,
              'issues': [], 'nodes': [node(1, 'AXWindow', 'Example', [2, 3]),
                                     node(2, 'AXButton', 'Find 🦀\n@e999'),
                                     node(3, 'AXButton', 'Cancel')]}
    after = copy.deepcopy(before)
    after['revision'] = 2
    after['nodes'][1]['attributes']['AXTitle']['value'] = 'Found 🦀'
    with tempfile.TemporaryDirectory(prefix='cli-') as directory:
        a, b = Path(directory)/'before.json', Path(directory)/'after.json'
        a.write_text(json.dumps(before))
        b.write_text(json.dumps(after))
        assert json.loads(run('view', a, '--format', 'json')) == before
        text = run('view', a, '--interactive')
        assert '- @e2' in text and '\\n@e999' in text and '\n@e999' not in text
        assert '🦀' in text
        compact = json.loads(run('view', a, '--format', 'compact', '--interactive', '--limit', '1'))
        assert compact['rows'][0]['reference'] == ref(2)
        assert compact['truncated_nodes'] == 1 and compact['filtered_nodes'] == 1
        scoped = run('view', a, '--root', '@e3')
        assert '- @e3' in scoped and '- @e2' not in scoped
        delta = run('diff', a, b, '--format', 'text')
        assert 'modified @e2 name' in delta and 'Found 🦀' in delta
        assert '\\\"omitted_chars' not in delta
        compact_delta = json.loads(run('diff', a, b, '--format', 'compact'))
        assert compact_delta['text'] == delta and compact_delta['before_revision'] == 1
        assert run('view', a) == run('view', a, '--format', 'text')
        assert run('diff', a, b) == delta
        assert json.loads(run('--json', 'view', a)) == before
        assert not run('query', a, '--name', 'Find').lstrip().startswith('{')
        native_delta = json.loads(run('diff', a, b, '--json'))
        assert native_delta['modified'][0]['reference'] == ref(2)
        # Windows/AT-SPI records must remain usable through the same offline CLI.
        desktop = copy.deepcopy(before)
        desktop['nodes'][0]['attributes'] = {'role': 'application', 'name': 'Editor'}
        desktop['nodes'][1]['attributes'] = {'role': 'button', 'name': 'Native Save', 'enabled': True, 'offscreen': False}
        desktop['nodes'][1]['actions'] = ['invoke']
        desktop['nodes'][2]['attributes'] = {'role': 'button', 'name': 'Hidden Save', 'offscreen': True}
        desktop['nodes'][2]['actions'] = ['invoke']
        a.write_text(json.dumps(desktop))
        text = run('view', a, '--interactive', '--hide-hidden')
        assert 'Native Save' in text and 'Hidden Save' not in text
        queried = json.loads(run('query', a, '--name', 'Native Save', '--role', 'button', '--json'))
        assert queried['matches'][0]['reference'] == ref(2)
        assert json.loads(run('view', a, '--json')) == desktop
        invalid = subprocess.run([BIN, 'view', str(a), '--format', 'typo'], capture_output=True)
        assert invalid.returncode != 0
    print('PASS CLI full JSON preservation, compact filtering, text escaping, scope, and projected/native diffs')


if __name__ == '__main__':
    main()
