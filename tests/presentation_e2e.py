"""Live scroll, compact snapshot, view diff, and SkyLight callback test on macOS.

Build the workspace and compile native/macos/scroll_fixture.swift to
native/macos/.build/scroll_fixture before running. Only the disposable fixture
receives input. Geometric viewport placement does not establish visibility.
"""
import json
import subprocess
import time

from macos_e2e import Session, value


def short(reference):
    return f"@e{reference['id']}"


def run():
    session = Session()
    fixture = subprocess.Popen(['native/macos/.build/scroll_fixture'])
    try:
        deadline = time.monotonic() + 8
        while True:
            reply = session.call(op='snapshot', request={'pid': fixture.pid},
                                 scope='focused_window', format='compact',
                                 options={'actionable_only': True})
            if 'result' in reply and any(row.get('name', {}).get('text') == 'Row 0'
                                         for row in reply['result']['rows']):
                before = reply['result']
                break
            assert time.monotonic() < deadline, reply
            time.sleep(.05)

        raw = session.result(op='view', revision=before['revision'], format='json')
        named = {value(node, 'AXIdentifier'): node for node in raw['nodes']}
        target = named['scroll-row-12']['reference']
        scroll = named['scroll-viewport']['reference']
        status = named['scroll-status']['reference']

        def row(view, reference):
            return next(r for r in view['rows'] if r['reference'] == reference)

        assert row(before, target)['viewport_relation'] == 'below', row(before, target)
        assert row(before, target)['scroll_container'] == scroll
        blocked = session.call(op='click', target=short(target), mode='skylight')
        assert blocked.get('error', {}).get('code') == 'outside_viewport', blocked
        assert blocked['error']['effect'] == 'none', blocked
        text = session.result(op='view', revision=before['revision'], format='text',
                              options={'actionable_only': True})
        assert short(target) in text and 'below' in text, text
        scoped = session.result(op='view', revision=before['revision'], format='compact',
                                options={'root': short(scroll), 'actionable_only': True})
        assert any(r['reference'] == target for r in scoped['rows'])
        print('PASS compact/text snapshots retain below-viewport buttons and scroll ancestry')

        after = before
        for _ in range(30):
            current = row(after, target)
            if current['viewport_relation'] == 'inside':
                break
            # SkyLight scroll deltas are pixels, not wheel-line counts.
            vertical = 80 if current['viewport_relation'] == 'above' else -80
            receipt = session.result(op='scroll_target', target=short(scroll),
                                     mode='skylight', vertical=vertical, horizontal=0)
            assert receipt['effect'] == 'dispatched', receipt
            time.sleep(.12)
            after = session.result(op='snapshot', request={'pid': fixture.pid},
                                   scope='focused_window', format='compact',
                                   options={'actionable_only': True})
        assert row(after, target)['viewport_relation'] == 'inside', row(after, target)
        first = named['scroll-row-0']['reference']
        assert row(after, first)['viewport_relation'] == 'above', row(after, first)
        delta = session.result(op='diff_view', before=before['revision'],
                               after=after['revision'], options={'actionable_only': True},
                               max_changes=100)
        assert short(target) in delta, delta
        assert 'inside' in delta and 'below' in delta, delta
        print('PASS SkyLight scrolling changes below to inside and above, with stable references and compact diff')

        receipt = session.result(op='click', target=short(target), mode='skylight')
        assert receipt['effect'] == 'dispatched', receipt
        deadline = time.monotonic() + 3
        while True:
            result = session.result(op='attribute', target=short(status), name='AXValue')
            if result.get('value') == 'clicked:12':
                break
            assert time.monotonic() < deadline, result
            time.sleep(.03)
        latest = session.result(op='snapshot', request={'pid': fixture.pid},
                                scope='focused_window', format='json')
        target_after = next(n for n in latest['nodes']
                            if value(n, 'AXIdentifier') == 'scroll-row-12')
        assert target_after['reference'] == target
        print('PASS short-reference SkyLight click consumed by newly scrolled-into-view native button')
        request = {'id': 'transport', 'op': 'observe', 'request': {'pid': fixture.pid}}
        for output_format in ['text', 'compact']:
            output = subprocess.check_output(['target/debug/actuate', 'session', '--format', output_format],
                                             input=json.dumps(request)+'\n', text=True, timeout=30)
            if output_format == 'text':
                assert '--- response id="transport" ---' in output and '- @e' in output
                assert '"attributes":' not in output
            else:
                reply = json.loads(output)
                assert reply['id'] == 'transport' and 'rows' in reply['result']
        print('PASS optional session text framing and compact JSONL transport')
    finally:
        fixture.terminate()
        try:
            fixture.wait(timeout=3)
        except subprocess.TimeoutExpired:
            fixture.kill()
            fixture.wait()
        session.close()


if __name__ == '__main__':
    run()
