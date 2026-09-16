#!/usr/bin/env python3
"""Drive two real desktop applications through one long-lived Linux session.

The calculator (Qt Quick `omacalc` by default) exposes no accessible children, so
it is driven by pixel clicks on an X11 window capture over the XTest route, which is
local to Xwayland and never moves the shared cursor. The terminal is driven by
window-targeted Hyprland shortcuts, which never change focus. The in-process soft
cursor follows every step, and the session stays open afterwards so the cursor can
be watched on the applications' workspace.

Run with both applications already open on a workspace of your own:

    ACTUATE_CALC_CLASS=omacalc ACTUATE_TERMINAL_TITLE=ActuateTerminal \
        python3 tests/linux_apps_demo.py

Nothing here dispatches seat input; the user can keep typing and pointing elsewhere.
"""
import json
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.environ.get('ACTUATE_DEMO_OUT', os.path.join(ROOT, 'target', 'linux-apps-demo'))
CALC_CLASS = os.environ.get('ACTUATE_CALC_CLASS', 'omacalc')
TERMINAL_TITLE = os.environ.get('ACTUATE_TERMINAL_TITLE', 'ActuateTerminal')
HOLD_SECONDS = int(os.environ.get('ACTUATE_DEMO_HOLD_SECONDS', '600'))
BINARY = os.environ.get('ACTUATE_BINARY', os.path.join(ROOT, 'target', 'debug', 'actuate'))

# Pixel centers of omacalc's keys in its 400x568 window capture at scale 1.25.
COLS = [60, 153, 246, 339]
ROWS = {'top': 220, '7': 295, '4': 369, '1': 443, '0': 517}
KEYS = {
    'AC': (COLS[0], ROWS['top']), '/': (COLS[3], ROWS['top']),
    '7': (COLS[0], ROWS['7']), '8': (COLS[1], ROWS['7']), '9': (COLS[2], ROWS['7']), '*': (COLS[3], ROWS['7']),
    '4': (COLS[0], ROWS['4']), '5': (COLS[1], ROWS['4']), '6': (COLS[2], ROWS['4']), '-': (COLS[3], ROWS['4']),
    '1': (COLS[0], ROWS['1']), '2': (COLS[1], ROWS['1']), '3': (COLS[2], ROWS['1']), '+': (COLS[3], ROWS['1']),
    '0': (COLS[0], ROWS['0']), '=': (COLS[3], ROWS['0']),
}
KEYSYMS = {' ': 'space', '-': 'minus'}


def log(*parts):
    print(*parts, flush=True)


def hypr(command):
    return json.loads(subprocess.check_output(['hyprctl', '-j', command], text=True))


class Session:
    def __init__(self):
        env = dict(os.environ, ACTUATE_SOFT_CURSOR='1')
        self.proc = subprocess.Popen([BINARY, 'session', '--json'], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True, bufsize=1, env=env)

    def __call__(self, **request):
        self.proc.stdin.write(json.dumps(request) + '\n')
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        if 'error' in reply:
            raise RuntimeError(f"{request.get('op')}: {reply['error']}")
        return reply['result']

    def close(self):
        self.proc.stdin.close()
        self.proc.wait()


def capture(session, source, name):
    path = os.path.join(OUT, f'{name}-{int(time.time() * 1000)}.png')
    return session(op='capture', source=source, path=path)


def main():
    os.makedirs(OUT, exist_ok=True)
    clients = hypr('clients')
    calc = next((c for c in clients if c['class'] == CALC_CLASS), None)
    term = next((c for c in clients if c['title'] == TERMINAL_TITLE), None)
    if calc is None or term is None:
        sys.exit(f'need a {CALC_CLASS!r} window and a window titled {TERMINAL_TITLE!r}')
    log('calculator', calc['pid'], calc['at'], calc['size'], 'xwayland' if calc['xwayland'] else 'wayland')
    log('terminal', term['pid'], term['address'], term['at'], term['size'])
    session = Session()
    try:
        # Terminal: keys go to the window through Hyprland; focus stays where the user has it.
        snapshot = session(op='snapshot', request={'pid': term['pid'], 'max_nodes': 200}, format='json')
        frame = next(n for n in snapshot['nodes'] if n['attributes'].get('role', {}).get('value') == 'frame')
        for char in list('echo actuate-ok') + ['Return']:
            session(op='hyprland_shortcut', target=frame['reference'], key=KEYSYMS.get(char, char))
            time.sleep(0.1)
        time.sleep(0.8)
        shot = capture(session, {'kind': 'window', 'address': term['address']}, 'terminal')
        log('terminal capture', shot['frame']['path'])
        log('active window after typing:', hypr('activewindow').get('title'))

        # Calculator: pixel clicks on an X11 capture, delivered by XTest inside Xwayland.
        session(op='input_route', route='x11')
        window = next(w for w in session(op='windows')['x11'] if w['pid'] == calc['pid'])
        source = {'kind': 'x11_window', 'window_id': window['window_id']}

        def press(key):
            frame = capture(session, source, f'calc-before-{key}')
            receipt = session(op='click_image', frame=frame['frame_id'], point=dict(zip('xy', KEYS[key])), mode='global')
            log('press', key, receipt['route'])
            time.sleep(0.35)

        for key in ['AC', '2', '+', '3', '=']:
            press(key)
        log('calculator after 2+3=', capture(session, source, 'calc-5')['frame']['path'])
        for key in ['*', '8', '=']:
            press(key)
        log('calculator after *8=', capture(session, source, 'calc-40')['frame']['path'])
        log('cursor', session(op='cursor_state'))
        log(f'holding the session and its soft cursor for {HOLD_SECONDS}s')
        time.sleep(HOLD_SECONDS)
        session(op='cursor_overlay', action={'kind': 'stop'})
    finally:
        session.close()


if __name__ == '__main__':
    main()
