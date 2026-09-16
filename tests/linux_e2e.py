"""Live Linux validation against the disposable GTK4 fixture on Hyprland.

Launch the fixtures first (see docs/linux.md); this script never focuses,
raises or moves windows and only dispatches global input when the fixture's
workspace is the active one. Set UNIMATION_FIXTURE_LOG and, for the X11
fixture, UNIMATION_FIXTURE_X11_LOG to the log files the fixtures append to.
"""
import json
import os
import selectors
import subprocess
import sys
import tempfile
import time

BIN = 'target/debug/unimation'
LOG = os.environ.get('UNIMATION_FIXTURE_LOG')
X11_LOG = os.environ.get('UNIMATION_FIXTURE_X11_LOG')


class Session:
    def __init__(self):
        self.p = subprocess.Popen([BIN, 'session', '--json'], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.p.stdout, selectors.EVENT_READ)

    def call(self, **request):
        self.p.stdin.write(json.dumps(request) + '\n')
        self.p.stdin.flush()
        if not self.selector.select(60):
            raise TimeoutError('Session did not respond within 60s')
        return json.loads(self.p.stdout.readline())

    def result(self, **request):
        reply = self.call(**request)
        assert 'error' not in reply, reply
        return reply['result']

    def close(self):
        self.p.stdin.close()
        try:
            self.p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
        self.selector.close()


def hyprctl(*args):
    return json.loads(subprocess.check_output(['hyprctl', '-j', *args], text=True))


def log_tail(path):
    if not path or not os.path.exists(path):
        return []
    with open(path, encoding='utf-8') as f:
        return [line.strip() for line in f if line.strip()]


def wait_log(path, needle, timeout=5.0, after=0):
    """Wait for an exact log line, optionally only among lines after index `after`."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if needle in log_tail(path)[after:]:
            return
        time.sleep(0.05)
    raise AssertionError(f'{needle!r} not found after line {after} in {path}: {log_tail(path)[after:][-5:]}')


def last_count(path, prefix):
    counts = [int(line[len(prefix):]) for line in log_tail(path) if line.startswith(prefix) and line[len(prefix):].isdigit()]
    return counts[-1] if counts else 0


def value(node, key):
    return node['attributes'].get(key, {}).get('value')


def fixture_clients():
    # Xwayland reports the interpreter name as the class; match the title there.
    return [c for c in hyprctl('clients')
            if c['class'].startswith('dev.unimation.fixture') or (c['xwayland'] and c['title'].startswith('Unimation Fixture'))]


def test_protocol(s):
    reply = s.call(op='discover', id='correlation-check')
    assert reply['id'] == 'correlation-check', reply
    assert reply['result']['accessibility_trusted'] is True, reply
    reply = s.call(op='pointer', delivery={'kind': 'global'}, action={'kind': 'click', 'point': {'x': 0, 'y': 0}, 'button': 'unsupported_button'}, id=2)
    assert reply['error']['code'] == 'invalid_request' and reply['id'] == 2, reply
    reply = s.call(op='inspect', target={'session': 'foreign', 'id': 1})
    assert reply['error']['code'] == 'stale_reference', reply
    s.p.stdin.write('invalid json\n')
    s.p.stdin.flush()
    assert s.selector.select(3)
    assert json.loads(s.p.stdout.readline())['error']['code'] == 'invalid_request'
    caps = s.result(op='capabilities')
    assert caps['observation'] == 'atspi' and caps['hyprland'] is True, caps
    print('PASS protocol: ids echoed, strict parsing, foreign references, recovery, capabilities')


def find(nodes, role, name=None):
    for n in nodes:
        if value(n, 'role') == role and (name is None or value(n, 'name') == name):
            return n
    raise AssertionError(f'no {role} {name!r} among {[ (value(n, "role"), value(n, "name")) for n in nodes][:40]}')


def find_any(nodes, roles, name=None):
    for role in roles:
        try:
            return find(nodes, role, name)
        except AssertionError:
            continue
    raise AssertionError(f'none of {roles} found')


def focus_check(s, node, log, label):
    """Focus a widget without activating its window: GrabFocus where the
    toolkit implements it, otherwise walk the window's focus chain with
    window-targeted Shift+Tab until the fixture reports the focus change."""
    reply = s.call(op='semantic', target=node['reference'], action={'kind': 'perform', 'name': 'component.grab_focus'})
    if 'error' not in reply:
        return
    assert reply['error']['code'] == 'unsupported' and reply['error']['effect'] == 'none', reply
    for _ in range(48):
        before = len(log_tail(log))
        s.result(op='hyprland_shortcut', target=node['reference'], mods='SHIFT', key='Tab')
        deadline = time.monotonic() + 1.0
        while time.monotonic() < deadline:
            new = log_tail(log)[before:]
            if any(line.startswith('focus:') for line in new):
                break
            time.sleep(0.02)
        if f'focus:{label}' in log_tail(log)[before:]:
            return
    raise AssertionError(f'focus never reached {label!r}: {log_tail(log)[-4:]}')


def test_semantic(s, client, log):
    pid = client['pid']
    snap = s.result(op='snapshot', request={'pid': pid}, format='json')
    nodes = snap['nodes']
    assert snap['traversal_complete'], snap['issues']
    frame = find(nodes, 'frame', 'Unimation Fixture')
    bounds = frame['attributes']['bounds']
    assert bounds['x'] == client['at'][0] and bounds['y'] == client['at'][1], (bounds, client['at'])
    assert value(frame, 'bounds_source').startswith('window_extents_plus_hyprland'), frame['attributes'].get('bounds_source')
    button = find(nodes, 'push button', 'Increment')
    click_action = next(a for a in button['actions'] if a.lower() == 'click')
    reply = s.call(op='semantic', target=button['reference'], action={'kind': 'perform', 'name': 'not-an-action'})
    assert reply['error']['code'] == 'unsupported', reply
    clicks = last_count(log, 'clicked:')
    receipt = s.result(op='click', target=button['reference'], mode='semantic')
    assert receipt['effect'] == 'dispatched' and receipt['route'] == 'linux.atspi.action', receipt
    wait_log(log, f'clicked:{clicks + 1}')
    b = button['attributes']['bounds']
    assert_cursor_followed(client, (b['x'] + b['width'] / 2, b['y'] + b['height'] / 2))
    receipt = s.result(op='semantic', target=button['reference'], action={'kind': 'perform', 'name': click_action})
    wait_log(log, f'clicked:{clicks + 2}')
    entry = find_any(nodes, ['text', 'entry'])
    receipt = s.result(op='semantic', target=entry['reference'], action={'kind': 'set_string', 'attribute': 'text', 'value': 'Hi🦀'})
    assert receipt['route'] == 'linux.atspi.editable_text.set_text_contents', receipt
    wait_log(log, 'entry:Hi🦀')
    text = s.result(op='attribute', target=entry['reference'], name='Text.Text')
    assert text == {'type': 'string', 'value': 'Hi🦀'}, text
    check = find(nodes, 'check box', 'Enable feature')
    was_checked = value(check, 'state.checked')
    assert was_checked in (True, False), check['attributes'].get('state.checked')
    mark = len(log_tail(log))
    if any(a.lower() in ('click', 'toggle', 'press') for a in check['actions']):
        s.result(op='click', target=check['reference'], mode='semantic')
    else:
        # GTK 4.22 advertises no toggle action on check buttons; focus it and
        # send Space through the window-targeted Hyprland route instead.
        reply = s.call(op='click', target=check['reference'], mode='semantic')
        assert reply['error']['code'] == 'unsupported', reply
        focus_check(s, check, log, 'Enable feature')
        s.result(op='hyprland_shortcut', target=check['reference'], key='space')
    wait_log(log, f'checked:{not was_checked}', after=mark)
    after = s.result(op='observe_subtree', target=check['reference'])
    assert value(after['nodes'][0], 'state.checked') is (not was_checked), after['nodes'][0]['attributes'].get('state.checked')
    slider = find(nodes, 'slider')
    current = value(slider, 'value')
    assert isinstance(current, float), slider['attributes'].get('value')
    target_value = (int(current) + 17) % 100
    s.result(op='semantic', target=slider['reference'], action={'kind': 'set_float', 'attribute': 'value', 'value': target_value})
    wait_log(log, f'slider:{target_value}')
    waited = s.result(op='wait_attribute', target=slider['reference'], name='Value.CurrentValue', expected={'type': 'float', 'value': float(target_value)}, timeout_ms=2000)
    assert waited['matched'], waited
    query = s.result(op='query', revision=snap['revision'], query={'role': {'kind': 'exact', 'value': 'push button'}, 'name': {'kind': 'contains', 'value': 'Incr'}})
    assert [m['reference'] for m in query['matches']] == [button['reference']], query
    diffed = s.result(op='snapshot', request={'pid': pid}, format='json')
    diff = s.result(op='diff', before=snap['revision'], after=diffed['revision'])
    changed = {m['reference']['id'] for m in diff['modified']}
    assert check['reference']['id'] in changed and slider['reference']['id'] in changed, diff['modified'][:3]
    view = s.result(op='diff_view', before=snap['revision'], after=diffed['revision'])
    assert f'slider:{target_value}' in view, view
    text_view = s.result(op='view', revision=diffed['revision'], format='text', options={'actionable_only': True})
    assert '"push button" "Increment"' in text_view, text_view
    window = s.result(op='window', target=button['reference'])
    assert window['address'] == client['address'], window
    actionability = s.result(op='actionability', target=button['reference'])
    assert actionability['native']['sensitive']['value'] is True, actionability
    print('PASS semantic: click, set text, toggle, slider, wait, query, diff, views, window, actionability')
    return nodes


def test_hyprland_shortcut(s, client, nodes, log):
    entry = find_any(nodes, ['text', 'entry'])
    focus_check(s, entry, log, 'GtkText')
    focused = hyprctl('activewindow')
    mark = len(log_tail(log))
    receipt = s.result(op='hyprland_shortcut', target=entry['reference'], mods='', key='z')
    assert receipt['route'] == 'linux.hyprland.send_shortcut', receipt
    # Keyboard focus selects the entry's text, so the key replaces or appends.
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if any(line.startswith('entry:') and line.endswith('z') for line in log_tail(log)[mark:]):
            break
        time.sleep(0.05)
    else:
        raise AssertionError(log_tail(log)[mark:][-5:])
    after = hyprctl('activewindow')
    assert focused.get('address') == after.get('address'), (focused.get('title'), after.get('title'))
    print('PASS hyprland_shortcut: key reached the fixture entry without changing the active window')


def test_capture(s, client, tmp):
    frame = s.result(op='capture', source={'kind': 'window', 'address': client['address']}, path=os.path.join(tmp, 'window.png'))
    mapping = frame['frame']['mapping']
    assert mapping['pixel_width'] > 0 and frame['frame']['route'] == 'linux.wayland.image_copy_capture', frame
    assert os.path.getsize(frame['frame']['path']) > 1000
    outputs = s.result(op='displays')['outputs']
    out = s.result(op='capture', source={'kind': 'output', 'name': outputs[0]['name']}, path=os.path.join(tmp, 'output.png'))
    assert out['frame']['mapping']['source_bounds']['width'] == outputs[0]['width'], out
    region = s.result(op='capture', source={'kind': 'region', 'x': client['at'][0], 'y': client['at'][1], 'width': 100, 'height': 50}, path=os.path.join(tmp, 'region.png'))
    assert region['frame']['mapping']['pixel_width'] >= 100, region
    reply = s.call(op='click_image', frame=frame['frame_id'], point={'x': 10, 'y': 10}, mode='semantic')
    assert reply['error']['code'] == 'unsupported', reply
    print('PASS capture: window, output and region frames with geometry mappings')
    return frame['frame_id']


def layer_namespaces():
    layers = hyprctl('layers')
    names = []
    for monitor in layers.values():
        for level in monitor.get('levels', {}).values():
            names.extend(layer.get('namespace') for layer in level)
    return names


def test_overlay(s):
    state = s.result(op='cursor_overlay', action={'kind': 'start'})
    assert state['overlay_running'] is True, state
    time.sleep(0.3)
    status = s.result(op='cursor_state')
    assert status['overlay_running'] is True and status['overlay_error'] is None, status
    assert 'unimation-cursor' in layer_namespaces(), layer_namespaces()
    s.result(op='cursor_overlay', action={'kind': 'stop'})
    time.sleep(0.3)
    assert 'unimation-cursor' not in layer_namespaces(), layer_namespaces()
    helper = os.path.abspath('target/debug/unimation-overlay')
    state = s.result(op='cursor_overlay', action={'kind': 'start', 'executable': helper})
    assert state['overlay_running'] is True, state
    time.sleep(0.5)
    status = s.result(op='cursor_state')
    assert status['overlay_running'] is True and status['overlay_pid'] not in (None, os.getpid()), status
    assert 'unimation-cursor' in layer_namespaces(), layer_namespaces()
    print('PASS overlay: in-process and helper-process layer-shell renderers registered compositor layer surfaces without input side effects')


def cursor_layer():
    for monitor in hyprctl('layers').values():
        for level in monitor.get('levels', {}).values():
            for layer in level:
                if layer.get('namespace') == 'unimation-cursor':
                    return layer
    return None


def assert_cursor_followed(client, point):
    """The window-scoped glyph sits at the target while its workspace is shown
    and is parked off-screen otherwise, so a person on another workspace never
    sees it."""
    time.sleep(0.4)
    layer = cursor_layer()
    assert layer is not None, 'no cursor layer surface'
    if hyprctl('activeworkspace')['id'] == client['workspace']['id']:
        assert abs(layer['x'] + 32 - point[0]) <= 2 and abs(layer['y'] + 32 - point[1]) <= 2, (layer, point)
    else:
        assert layer['x'] < 0 or layer['y'] < 0, layer


def test_global_if_active(s, client, nodes, log, frame_id):
    active = hyprctl('activeworkspace')['id']
    button = find(nodes, 'push button', 'Increment')
    if os.environ.get('UNIMATION_ALLOW_GLOBAL_INPUT') != '1':
        print('SKIP global pointer: set UNIMATION_ALLOW_GLOBAL_INPUT=1 to move the shared cursor when the fixture workspace is active')
        return
    if active != client['workspace']['id']:
        reply = s.call(op='click', target=button['reference'], mode='global')
        assert reply['error']['code'] == 'not_on_active_workspace', reply
        cursor = s.result(op='cursor_state')
        print(f'SKIP global pointer: fixture workspace {client["workspace"]["id"]} is not active ({active}); guard refused dispatch with no effect; shared cursor {cursor["shared_cursor"]}')
        return
    before = hyprctl('cursorpos')
    clicks = last_count(log, 'clicked:')
    receipt = s.result(op='click', target=button['reference'], mode='global')
    assert receipt['route'] == 'linux.wayland.virtual_pointer', receipt
    wait_log(log, f'clicked:{clicks + 1}')
    reply = s.call(op='click_image', frame=frame_id, point={'x': 10, 'y': 10}, mode='global')
    assert reply['error']['code'] == 'stale_frame', reply
    print(f'PASS global pointer on active workspace: cursor moved from {before} to {hyprctl("cursorpos")}')


def test_x11(s):
    clients = [c for c in fixture_clients() if c['xwayland']]
    if not clients or not X11_LOG:
        print('SKIP x11: no Xwayland fixture running (launch fixture.py with GDK_BACKEND=x11 and UNIMATION_FIXTURE_ID)')
        return
    client = clients[0]
    s.result(op='input_route', route='x11')
    snap = s.result(op='snapshot', request={'pid': client['pid']}, format='json')
    button = find(snap['nodes'], 'push button', 'Increment')
    clicks = last_count(X11_LOG, 'clicked:')
    before = hyprctl('cursorpos')
    receipt = s.result(op='click', target=button['reference'], mode='global')
    assert receipt['route'] == 'linux.x11.xtest.pointer.xwayland_local', receipt
    wait_log(X11_LOG, f'clicked:{clicks + 1}')
    after = hyprctl('cursorpos')
    # A person may be moving the mouse; what must not happen is the XTest
    # target pulling the shared cursor onto the button.
    bounds = button['attributes']['bounds']
    center = (bounds['x'] + bounds['width'] / 2, bounds['y'] + bounds['height'] / 2)
    moved_to_target = abs(after['x'] - center[0]) < 3 and abs(after['y'] - center[1]) < 3
    assert not moved_to_target or (abs(before['x'] - center[0]) < 3 and abs(before['y'] - center[1]) < 3), (before, after, center)
    entry = find_any(snap['nodes'], ['text', 'entry'])
    receipt = s.result(op='click_window', target=entry['reference'], point={'x': 5, 'y': 5}, mode='process')
    assert receipt['route'] == 'linux.x11.send_event', receipt
    windows = s.result(op='windows')['x11']
    assert any(w['pid'] == client['pid'] for w in windows), windows
    x11_window = next(w for w in windows if w['pid'] == client['pid'])
    with tempfile.TemporaryDirectory(prefix='unimation-x11-') as tmp:
        frame = s.result(op='capture', source={'kind': 'x11_window', 'window_id': x11_window['window_id']}, path=os.path.join(tmp, 'x11.png'))
        assert frame['frame']['route'] == 'linux.x11.get_image', frame
    s.result(op='input_route', route='wayland')
    print(f'PASS x11: XTest click reached the Xwayland fixture while the shared cursor stayed at {before}; window capture through GetImage')


def main():
    clients = [c for c in fixture_clients() if not c['xwayland']]
    if not clients or not LOG:
        sys.exit('Launch native/linux/fixture.py with UNIMATION_FIXTURE_LOG first')
    client = clients[0]
    s = Session()
    try:
        test_protocol(s)
        test_overlay(s)
        nodes = test_semantic(s, client, LOG)
        test_hyprland_shortcut(s, client, nodes, LOG)
        status = s.result(op='cursor_state')
        assert status['overlay_running'] is True and status['overlay_error'] is None, status
        with tempfile.TemporaryDirectory(prefix='unimation-linux-') as tmp:
            frame_id = test_capture(s, client, tmp)
            test_global_if_active(s, client, nodes, LOG, frame_id)
        test_x11(s)
        s.result(op='cursor_overlay', action={'kind': 'stop'})
    finally:
        s.close()
    print('Linux end-to-end checks passed')


if __name__ == '__main__':
    main()
