"""Live SkyLight checks using only Calculator and the disposable native fixture.

Build the CLI and native/macos/.build/fixture first. Accessibility access is
required. Run from the repository root while leaving the mouse untouched.
"""
import ctypes
import subprocess
import time
import os
import tempfile
from pathlib import Path
from macos_e2e import Session, value


class CGPoint(ctypes.Structure):
    _fields_ = [('x', ctypes.c_double), ('y', ctypes.c_double)]


def cursor_position():
    cg = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
    cf = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
    cg.CGEventCreate.argtypes = [ctypes.c_void_p]
    cg.CGEventCreate.restype = ctypes.c_void_p
    cg.CGEventGetLocation.argtypes = [ctypes.c_void_p]
    cg.CGEventGetLocation.restype = CGPoint
    cf.CFRelease.argtypes = [ctypes.c_void_p]
    event = cg.CGEventCreate(None)
    assert event, 'Could not read the cursor position'
    try:
        point = cg.CGEventGetLocation(event)
        return point.x, point.y
    finally:
        cf.CFRelease(event)


def active_pids(session):
    apps = session.result(op='discover')['applications']
    # AXFrontmost is also true for auxiliary UI processes. Preserve the whole
    # observed set instead of inventing one authoritative frontmost PID.
    return frozenset(app['pid'] for app in apps if app['active'])


def wait_for(predicate, explanation):
    deadline = time.monotonic() + 5
    while not predicate():
        if time.monotonic() >= deadline:
            raise AssertionError(explanation)
        time.sleep(.05)


def test_event_variants():
    # The additional native view records the events it actually receives. The
    # production backend has no Swift runtime or helper dependency.
    source = Path('native/macos/fixture.swift').read_text()
    handlers = r"""
    var field: NSTextField!
    override func mouseDown(with event: NSEvent) { field.stringValue = "left:\(event.clickCount):\(event.modifierFlags.contains(.shift))" }
    override func mouseUp(with event: NSEvent) { field.stringValue += "|up" }
    override func rightMouseDown(with event: NSEvent) { field.stringValue = "right" }
    override func rightMouseUp(with event: NSEvent) { field.stringValue += "|up" }
    override func otherMouseDown(with event: NSEvent) { field.stringValue = "middle" }
    override func otherMouseUp(with event: NSEvent) { field.stringValue += "|up" }
    override func mouseDragged(with event: NSEvent) { field.stringValue = "drag" }
"""
    with tempfile.TemporaryDirectory(prefix='actuate-events-') as directory:
        swift = Path(directory) / 'probe.swift'
        binary = Path(directory) / 'probe'
        swift.write_text(source.replace('    var field: NSTextField!', handlers, 1))
        env = dict(os.environ)
        env.setdefault('DEVELOPER_DIR', '/Library/Developer/CommandLineTools')
        subprocess.run(['swiftc', str(swift), '-o', str(binary)], env=env, check=True)
        process = subprocess.Popen([str(binary)])
        session = Session()
        try:
            tree = None
            def ready():
                nonlocal tree
                tree = session.result(op='observe', request={'pid': process.pid})
                return any(value(n, 'AXIdentifier') == 'actuate-test-field' for n in tree['nodes'])
            wait_for(ready, 'Event probe did not appear')
            def activate():
                session.result(op='semantic', target=tree['root'],
                               action={'kind': 'set_bool', 'attribute': 'AXFrontmost', 'value': True})
                return process.pid in active_pids(session)
            wait_for(activate, 'Event probe did not activate')
            nodes = {value(n, 'AXIdentifier'): n for n in tree['nodes']}
            field = nodes['actuate-test-field']['reference']
            view = nodes['actuate-test-scroll']
            target = view['reference']
            cases = [('left', 1, {}, 'left:1:false|up'),
                     ('left', 2, {}, 'left:2:false|up'),
                     ('left', 3, {'shift': True}, 'left:3:true|up'),
                     ('right', 1, {}, 'right|up'), ('middle', 1, {}, 'middle|up')]
            for button, count, modifiers, expected in cases:
                session.result(op='click', target=target, mode='skylight', button=button,
                               count=count, modifiers=modifiers)
                wait_for(lambda: session.result(op='attribute', target=field, name='AXValue')['value'] == expected,
                         f'Native event mismatch for {button} count {count}')
            position, size = view['attributes']['AXPosition'], view['attributes']['AXSize']
            point = {'x': position['x'] + size['width']/2, 'y': position['y'] + size['height']/2}
            session.result(op='skylight_pointer', target=target,
                           action={'kind': 'drag', 'from': point,
                                   'to': {'x': point['x']+30, 'y': point['y']}, 'duration_ms': 250})
            wait_for(lambda: session.result(op='attribute', target=field, name='AXValue')['value'] == 'drag|up',
                     'Native drag motion and release were not received')
            print('PASS native event evidence for click counts, Shift, right/middle buttons, drag motion and release')
        finally:
            process.terminate()
            process.wait(timeout=5)
            session.close()


def main():
    subprocess.run(['open', '-a', 'Calculator'], check=True)
    session = Session()
    fixture = subprocess.Popen(['native/macos/.build/fixture'])
    try:
        tree = None
        def loaded():
            nonlocal tree
            tree = session.result(op='observe', request={'pid': fixture.pid})
            return any(value(node, 'AXIdentifier') == 'actuate-test-field' for node in tree['nodes'])
        wait_for(loaded, 'Fixture did not expose its controls')
        refs = {value(node, 'AXIdentifier'): node['reference'] for node in tree['nodes']}
        field = refs['actuate-test-field']
        button = refs['actuate-test-button']
        scroll = refs['actuate-test-scroll']
        fixture_root = tree['root']

        def activate_fixture():
            session.result(op='semantic', target=fixture_root,
                           action={'kind': 'set_bool', 'attribute': 'AXFrontmost', 'value': True})
            return fixture.pid in active_pids(session)
        wait_for(activate_fixture, 'Fixture did not become the active application')

        def check_action(expected, request):
            session.result(op='semantic', target=field,
                           action={'kind': 'set_string', 'attribute': 'AXValue', 'value': 'reset'})
            cursor = cursor_position()
            active = active_pids(session)
            receipt = session.result(**request)
            assert receipt == {'effect': 'dispatched', 'route': 'macos.skylight.window'}, receipt
            wait_for(lambda: session.result(op='attribute', target=field, name='AXValue')['value'] == expected,
                     f'Input was dispatched but {expected!r} was not observed')
            assert cursor_position() == cursor, 'Shared cursor moved during targeted input'
            assert active_pids(session) == active, 'Active application changed during targeted input'

        def check_fixture():
            for count in (1, 2, 3):
                check_action('clicked', {'op': 'click', 'target': button, 'mode': 'skylight', 'count': count})
            check_action('clicked', {'op': 'click', 'target': button, 'mode': 'skylight', 'modifiers': {'shift': True}})
            for vertical, horizontal in ((5, 0), (-5, 0), (0, 5), (0, -5)):
                check_action('scrolled', {'op': 'scroll_target', 'target': scroll, 'mode': 'skylight',
                                          'vertical': vertical, 'horizontal': horizontal})
            current = session.result(op='observe', request={'pid': fixture.pid})
            current_refs = {value(node, 'AXIdentifier'): node['reference'] for node in current['nodes']}
            assert current_refs['actuate-test-button'] == button
            assert current_refs['actuate-test-field'] == field

        check_fixture()
        print('PASS foreground SkyLight click variants, modifier flags, both scroll axes, stable refs, cursor/focus unchanged')
        subprocess.run(['open', '-a', 'Calculator'], check=True)
        def calculator_active():
            apps = session.result(op='discover')['applications']
            calculator = next((app for app in apps if app['bundle_id'] == 'com.apple.calculator'), None)
            if not calculator:
                return False
            if calculator['active']:
                return True
            root = session.result(op='observe', request={'pid': calculator['pid']})['root']
            session.result(op='semantic', target=root,
                           action={'kind': 'set_bool', 'attribute': 'AXFrontmost', 'value': True})
            return False
        wait_for(calculator_active, 'Calculator did not activate')
        check_fixture()
        print('PASS background SkyLight click/scroll variants with Calculator remaining active and cursor unchanged')
        test_event_variants()
    finally:
        fixture.terminate()
        fixture.wait(timeout=5)
        session.close()


if __name__ == '__main__':
    main()
