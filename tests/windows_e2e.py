#!/usr/bin/env python3
"""Live Windows fixtures. No global input; all mutations target owned test apps."""
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import queue
import subprocess
import threading
import time
import traceback
import os
import sys
import uuid

ROOT = Path(__file__).resolve().parents[1]
USER = C.WinDLL('user32', use_last_error=True)
USER.GetForegroundWindow.restype = W.HWND
USER.GetWindow.argtypes = [W.HWND, W.UINT]
USER.GetWindow.restype = W.HWND
USER.GetWindowRect.argtypes = [W.HWND, C.POINTER(W.RECT)]
USER.IsWindowVisible.argtypes = [W.HWND]
USER.ShowWindow.argtypes = [W.HWND, C.c_int]
USER.SetWindowPos.argtypes = [W.HWND, W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.UINT]
USER.GetWindowLongW.argtypes = [W.HWND, C.c_int]
USER.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
USER.SetProcessDpiAwarenessContext.argtypes = [W.HANDLE]
USER.SetProcessDpiAwarenessContext(W.HANDLE(-4))
USER.CreateDesktopW.argtypes = [W.LPCWSTR, W.LPCWSTR, W.LPVOID, W.DWORD, W.DWORD, W.LPVOID]
USER.CreateDesktopW.restype = W.HANDLE
USER.CloseDesktop.argtypes = [W.HANDLE]
USER.GetThreadDesktop.argtypes = [W.DWORD]
USER.GetThreadDesktop.restype = W.HANDLE
USER.OpenInputDesktop.argtypes = [W.DWORD, W.BOOL, W.DWORD]
USER.OpenInputDesktop.restype = W.HANDLE
USER.GetUserObjectInformationW.argtypes = [W.HANDLE, C.c_int, W.LPVOID, W.DWORD, C.POINTER(W.DWORD)]


def desktop_name(handle):
    name = C.create_unicode_buffer(256)
    needed = W.DWORD()
    if not USER.GetUserObjectInformationW(handle, 2, name, C.sizeof(name), C.byref(needed)):
        raise C.WinError(C.get_last_error())
    return name.value


def assert_isolated():
    current = desktop_name(USER.GetThreadDesktop(C.windll.kernel32.GetCurrentThreadId()))
    interactive = USER.OpenInputDesktop(0, False, 1)
    if not interactive:
        raise C.WinError(C.get_last_error())
    try:
        assert current.startswith('ActuateTest-') and current != desktop_name(interactive), f'Refusing fixtures on desktop {current!r}; input desktop is {desktop_name(interactive)!r}'
    finally:
        USER.CloseDesktop(interactive)
    return current


def isolated_parent(script=None, log_path=None):
    name = 'ActuateTest-' + uuid.uuid4().hex
    desktop = USER.CreateDesktopW(name, None, None, 0, 0x01ff, None)
    if not desktop:
        raise C.WinError(C.get_last_error())
    # Python's subprocess.STARTUPINFO does not marshal lpDesktop. Use the native
    # structure so isolation is established before Python or a toolkit starts.
    class Startup(C.Structure):
        _fields_ = [('cb', W.DWORD), ('reserved', W.LPWSTR), ('desktop', W.LPWSTR),
                    ('title', W.LPWSTR), ('x', W.DWORD), ('y', W.DWORD),
                    ('cx', W.DWORD), ('cy', W.DWORD), ('chars_x', W.DWORD),
                    ('chars_y', W.DWORD), ('fill', W.DWORD), ('flags', W.DWORD),
                    ('show', W.WORD), ('reserved_size', W.WORD), ('reserved_ptr', W.LPVOID),
                    ('stdin', W.HANDLE), ('stdout', W.HANDLE), ('stderr', W.HANDLE)]
    class ProcessInfo(C.Structure):
        _fields_ = [('process', W.HANDLE), ('thread', W.HANDLE), ('pid', W.DWORD), ('tid', W.DWORD)]
    class BasicLimits(C.Structure):
        _fields_ = [('process_time', C.c_int64), ('job_time', C.c_int64),
                    ('flags', W.DWORD), ('min_working_set', C.c_size_t),
                    ('max_working_set', C.c_size_t), ('active_processes', W.DWORD),
                    ('affinity', C.c_size_t), ('priority', W.DWORD), ('scheduling', W.DWORD)]
    class ExtendedLimits(C.Structure):
        _fields_ = [('basic', BasicLimits), ('io', C.c_uint64 * 6),
                    ('process_memory', C.c_size_t), ('job_memory', C.c_size_t),
                    ('peak_process_memory', C.c_size_t), ('peak_job_memory', C.c_size_t)]
    kernel = C.WinDLL('kernel32', use_last_error=True)
    kernel.CreateProcessW.argtypes = [W.LPCWSTR, W.LPWSTR, W.LPVOID, W.LPVOID, W.BOOL, W.DWORD, W.LPVOID, W.LPCWSTR, C.POINTER(Startup), C.POINTER(ProcessInfo)]
    kernel.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
    kernel.GetExitCodeProcess.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
    kernel.CloseHandle.argtypes = [W.HANDLE]
    kernel.CreateJobObjectW.argtypes = [W.LPVOID, W.LPCWSTR]
    kernel.CreateJobObjectW.restype = W.HANDLE
    kernel.SetInformationJobObject.argtypes = [W.HANDLE, C.c_int, W.LPVOID, W.DWORD]
    kernel.AssignProcessToJobObject.argtypes = [W.HANDLE, W.HANDLE]
    kernel.ResumeThread.argtypes = [W.HANDLE]
    kernel.TerminateProcess.argtypes = [W.HANDLE, W.UINT]
    job = kernel.CreateJobObjectW(None, None)
    limits = ExtendedLimits(basic=BasicLimits(flags=0x2000))
    if not job or not kernel.SetInformationJobObject(job, 9, C.byref(limits), C.sizeof(limits)):
        if job:
            kernel.CloseHandle(job)
        USER.CloseDesktop(desktop)
        raise C.WinError(C.get_last_error())
    startup = Startup(cb=C.sizeof(Startup), desktop='winsta0\\' + name)
    process = ProcessInfo()
    try:
        log_path = log_path or ROOT / 'target/windows-e2e/isolated-worker.log'
        log_path.parent.mkdir(parents=True, exist_ok=True)
        command = C.create_unicode_buffer(subprocess.list2cmdline([sys.executable, str(Path(script or __file__).resolve()), *sys.argv[1:], '--isolated-worker']))
        if not kernel.CreateProcessW(None, command, None, None, False, subprocess.CREATE_NO_WINDOW | 4, None, str(ROOT), C.byref(startup), C.byref(process)):
            raise C.WinError(C.get_last_error())
        try:
            if not kernel.AssignProcessToJobObject(job, process.process):
                kernel.TerminateProcess(process.process, 1)
                raise C.WinError(C.get_last_error())
            assert kernel.ResumeThread(process.thread) != -1
            deadline = time.monotonic() + 180
            while kernel.WaitForSingleObject(process.process, 1000) == 258:
                if time.monotonic() >= deadline:
                    raise TimeoutError('Isolated suite exceeded 180 seconds; closing its process job')
            result = W.DWORD()
            assert kernel.GetExitCodeProcess(process.process, C.byref(result))
        finally:
            kernel.CloseHandle(process.thread)
            kernel.CloseHandle(process.process)
        print(log_path.read_text(encoding='utf-8'), end='')
        return result.value
    finally:
        kernel.CloseHandle(job)
        USER.CloseDesktop(desktop)


def desktop_state():
    point = W.POINT()
    available = USER.GetCursorPos(C.byref(point))
    return {'foreground': USER.GetForegroundWindow(), 'cursor': [point.x, point.y] if available else None}


def rect(hwnd):
    value = W.RECT()
    assert USER.GetWindowRect(hwnd, C.byref(value))
    return [value.left, value.top, value.right, value.bottom]


class Session:
    def __init__(self, output):
        self.output = output
        self.transcript = []
        self.log = (output / 'session.stderr').open('w', encoding='utf-8')
        self.process = subprocess.Popen(
            [str(ROOT / 'target/debug/actuate.exe'), '--provider', 'native', 'session', '--json'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
            text=True, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)
        self.queue = queue.Queue()
        def read():
            for line in self.process.stdout:
                self.queue.put(line)
            self.queue.put(None)
        threading.Thread(target=read, daemon=True).start()

    def call(self, op, allow_error=False, **fields):
        request = dict(id=len(self.transcript)+1, op=op, **fields)
        self.process.stdin.write(json.dumps(request)+'\n')
        self.process.stdin.flush()
        line = self.queue.get(timeout=35)
        assert line, 'Session exited'
        reply = json.loads(line)
        self.transcript.append({'request': request, 'reply': reply})
        assert reply['id'] == request['id']
        if allow_error:
            return reply
        assert 'error' not in reply, reply
        return reply['result']

    def snapshot(self, pid):
        return self.call('snapshot', request=dict(pid=pid, max_nodes=500, max_depth=25))

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.log.close()
        (self.output / 'transcript.json').write_text(json.dumps(self.transcript, indent=2), encoding='utf-8')


def find(snapshot, key, value, action=None):
    matches = [n for n in snapshot['nodes'] if n['attributes'].get(key) == value and (action is None or action in n['actions'])]
    assert len(matches) == 1, (key, value, len(matches))
    return matches[0]


def eventually(predicate, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError('Condition did not become true')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--electron', type=Path, help='Path to locally installed electron.exe')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/windows-e2e/results')
    parser.add_argument('--isolated-worker', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    results = {'checks': [], 'isolated_desktop': assert_isolated(), 'initial_desktop': desktop_state()}
    session = Session(args.output)
    children = []
    logs = []
    def check(name, action):
        try:
            evidence = action()
            results['checks'].append(dict(name=name, status='pass', evidence=evidence))
            print('PASS', name, flush=True)
        except Exception as error:
            results['checks'].append(dict(name=name, status='fail', error=str(error), traceback=traceback.format_exc()))
            print('FAIL', name, str(error), flush=True)
    def launch(name, command):
        desktop = assert_isolated()
        log = (args.output / (name+'.stderr')).open('w', encoding='utf-8')
        logs.append(log)
        environment = dict(os.environ, ACTUATE_ISOLATED_DESKTOP=desktop)
        environment.pop('ELECTRON_RUN_AS_NODE', None)
        child = subprocess.Popen(command, stdout=log, stderr=log, env=environment, creationflags=subprocess.CREATE_NO_WINDOW)
        children.append(child)
        return child
    def app_test(name, process):
        windows = eventually(lambda: [w for w in session.call('windows', allow_error=True).get('result', []) if w['pid'] == process.pid and w['visible']], timeout=20)
        before = desktop_state()
        assert before['foreground'] not in [w['window_id'] for w in windows], 'Fixture is foreground'
        snapshot = session.snapshot(process.pid)
        primary = next(w for w in windows if w['title'].startswith('Actuate'))
        scoped = session.call('snapshot',window_id=primary['window_id'],request=dict(pid=process.pid,max_nodes=500,max_depth=25))
        # Scoping does not make a native provider's tree complete. In particular,
        # System-menu children on an inactive desktop can lack runtime IDs.
        roots=scoped['nodes'][0]['children']
        assert len(roots)==1, roots
        native_root=next(n for n in scoped['nodes'] if n['reference']==roots[0])
        assert native_root['attributes']['native_window_handle']==primary['window_id']
        if not scoped['traversal_complete']:
            assert scoped['issues'] or any(n['issues'] for n in scoped['nodes']), 'Incomplete traversal must retain its errors'
        assert scoped['root'] != snapshot['root'], 'Window and process scopes must have distinct roots'
        mismatch = session.call('snapshot',allow_error=True,window_id=primary['window_id'],request=dict(pid=process.pid+1,max_nodes=500,max_depth=25))
        assert mismatch['error']['code']=='target_not_found'
        button = find(snapshot, 'name', 'Increment', 'invoke')
        editor = next(n for n in snapshot['nodes'] if 'set_value' in n['actions'] and (n['attributes'].get('automation_id') in ('editor', '102') or n['attributes'].get('name') == 'editor'))
        checkbox = find(snapshot, 'name', 'Option', 'toggle')
        assert checkbox['attributes']['toggle_state'] == 0
        session.call('semantic', target=button['reference'], action=dict(kind='perform', name='invoke'))
        eventually(lambda: any(n['attributes'].get('name') == 'count:1' for n in session.snapshot(process.pid)['nodes']))
        steps = {'before': before, 'after_invoke': desktop_state()}
        results.setdefault('action_desktop_states', {})[name] = steps
        value = 'Hi\u00f0\U0001f980'
        session.call('semantic', target=editor['reference'], action=dict(kind='set_string', attribute='value', value=value))
        eventually(lambda: session.call('inspect', target=editor['reference'])['attributes']['value'] == value)
        steps['after_set_value'] = desktop_state()
        session.call('semantic', target=checkbox['reference'], action=dict(kind='perform', name='toggle'))
        eventually(lambda: session.call('inspect', target=checkbox['reference'])['attributes']['toggle_state'] == 1)
        steps['after_toggle'] = desktop_state()
        after = session.snapshot(process.pid)
        assert find(after, 'name', 'Increment', 'invoke')['reference'] == button['reference']
        if any(state != before for state in steps.values()):
            raise AssertionError(steps)
        if name == 'wpf':
            eventually(lambda: len([w for w in session.call('windows') if w['pid'] == process.pid and w['title'].startswith('Actuate WPF')]) == 2)
            assert len([n for n in after['nodes'] if n['attributes'].get('name', '').startswith('Actuate WPF')]) == 2
        assert session.call('inspect', target='@e'+str(button['reference']['id']))['reference'] == button['reference']
        foreign = dict(button['reference'], session='foreign-test-session')
        assert session.call('inspect', target=foreign, allow_error=True)['error']['code'] == 'wrong_session'
        truncated = session.call('snapshot', request=dict(pid=process.pid, max_nodes=2, max_depth=25))
        assert len(truncated['nodes']) <= 2 and not truncated['traversal_complete']
        session.call('diff', before=snapshot['revision'], after=after['revision'])
        invalid = session.call('semantic', allow_error=True, target=button['reference'], action=dict(kind='perform', name='invalid_test_action'))
        assert invalid['error']['effect'] == 'none'
        rejected = session.call('text', allow_error=True, delivery=dict(kind='process', pid=process.pid), text='must not type')
        assert rejected['error']['effect'] == 'none'
        return dict(pid=process.pid, windows=windows, nodes=len(snapshot['nodes']), traversal_complete=snapshot['traversal_complete'], desktop_before=before, desktop_after=desktop_state())
    try:
        results['displays'] = session.call('displays')
        results['capabilities'] = session.call('capabilities')
        for toolkit in ['wpf', 'winforms']:
            child = launch(toolkit, ['powershell.exe', '-NoProfile', '-STA', '-ExecutionPolicy', 'Bypass', '-File', str(ROOT / 'tests/windows_fixture.ps1'), '-Toolkit', toolkit])
            check(toolkit+'_isolated_semantics', lambda t=toolkit, p=child: app_test(t, p))
        child = launch('win32', [sys.executable, str(ROOT / 'tests/windows_win32_fixture.py')])
        check('win32_isolated_semantics', lambda: app_test('win32', child))
        if args.electron:
            child = launch('electron', [str(args.electron.resolve()), str(ROOT / 'tests/windows_electron_fixture.cjs'), str((args.output / 'electron-profile').resolve())])
            check('electron_isolated_semantics', lambda: app_test('electron', child))
        else:
            results['checks'].append(dict(name='electron', status='skipped', reason='Supply --electron'))
        def overlay_test():
            target = next(w for w in session.call('windows') if w['pid'] == children[0].pid and w['title'] == 'Actuate WPF fixture')
            log = (args.output / 'overlay.stderr').open('w', encoding='utf-8')
            overlay = subprocess.Popen([str(ROOT / 'target/debug/actuate.exe'), 'overlay'], stdin=subprocess.PIPE, stderr=log, text=True, creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                for command in [dict(op='scope', scope=dict(kind='window', window_id=target['window_id'], pid=target['pid'])), dict(op='move', x=200, y=200, duration_ms=500)]:
                    overlay.stdin.write(json.dumps(command)+'\n')
                overlay.stdin.flush()
                time.sleep(1)
                assert overlay.poll() is None, 'Renderer exited on unavailable shell membership'
                windows = [w for w in session.call('windows') if w['pid'] == overlay.pid]
                assert windows and all(not w['visible'] for w in windows), windows
                return dict(hidden=True, reason='Separate Win32 desktop has no verified shell virtual-desktop membership; presentation must stay hidden')
            finally:
                if overlay.poll() is None:
                    overlay.stdin.write(json.dumps(dict(op='quit'))+'\n')
                    overlay.stdin.close()
                    try:
                        overlay.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        overlay.kill()
                        overlay.wait()
                log.close()
        check('overlay_hides_without_shell_membership_and_stays_alive', overlay_test)
        results['checks'].append(dict(name='overlay_visible_animation_and_task_view', status='skipped', reason='Requires an active compositor on a dedicated interactive test session'))
    finally:
        for child in children:
            for window in session.call('windows'):
                if window['pid'] == child.pid:
                    USER.PostMessageW(window['window_id'], 0x10, 0, 0)
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.terminate()
                child.wait(timeout=5)
        session.close()
        for log in logs:
            log.close()
        results['final_desktop'] = desktop_state()
        (args.output / 'results.json').write_text(json.dumps(results, indent=2), encoding='utf-8')
    raise SystemExit(int(any(c['status'] == 'fail' for c in results['checks'])))


if __name__ == '__main__':
    if '--isolated-worker' in sys.argv:
        sys.stdout = sys.stderr = (ROOT / 'target/windows-e2e/isolated-worker.log').open('w', encoding='utf-8', buffering=1)
        main()
    else:
        raise SystemExit(isolated_parent())
