"""Disposable native HWND controls, used only by the isolated Windows test runner."""
import ctypes as C
from ctypes import wintypes as W
from windows_e2e import assert_isolated

assert_isolated()
user = C.WinDLL('user32', use_last_error=True)
kernel = C.WinDLL('kernel32', use_last_error=True)
PROC = C.WINFUNCTYPE(C.c_ssize_t, W.HWND, W.UINT, W.WPARAM, W.LPARAM)


class WindowClass(C.Structure):
    _fields_ = [('style', W.UINT), ('proc', PROC), ('class_extra', C.c_int),
                ('window_extra', C.c_int), ('instance', W.HINSTANCE), ('icon', W.HICON),
                ('cursor', W.HANDLE), ('background', W.HBRUSH),
                ('menu', W.LPCWSTR), ('name', W.LPCWSTR)]


kernel.GetModuleHandleW.restype = W.HMODULE
user.DefWindowProcW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
user.DefWindowProcW.restype = C.c_ssize_t
user.CreateWindowExW.argtypes = [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD,
                               C.c_int, C.c_int, C.c_int, C.c_int, W.HWND,
                               W.HMENU, W.HINSTANCE, W.LPVOID]
user.CreateWindowExW.restype = W.HWND
user.ShowWindow.argtypes = [W.HWND, C.c_int]
user.SetWindowTextW.argtypes = [W.HWND, W.LPCWSTR]
counter = 0
status = None


@PROC
def window_proc(hwnd, message, wparam, lparam):
    global counter
    if message == 0x111 and wparam & 0xffff == 101:
        counter += 1
        user.SetWindowTextW(status, f'count:{counter}')
        return 0
    if message == 2:
        user.PostQuitMessage(0)
        return 0
    return user.DefWindowProcW(hwnd, message, wparam, lparam)


instance = kernel.GetModuleHandleW(None)
wc = WindowClass(proc=window_proc, instance=instance, background=6, name='ActuateNativeFixture')
assert user.RegisterClassW(C.byref(wc))
window = user.CreateWindowExW(0, wc.name, 'Actuate Win32 fixture', 0x00cf0000,
                            100, 100, 420, 320, None, None, instance, None)
assert window


def control(kind, title, extra, y, identifier):
    hwnd = user.CreateWindowExW(0, kind, title, 0x50000000 | extra,
                               15, y, 300, 30, window, identifier, instance, None)
    assert hwnd
    return hwnd


status = control('STATIC', 'count:0', 0, 10, 100)
control('BUTTON', 'Increment', 0, 50, 101)
control('EDIT', '', 0x00800000, 90, 102)
control('BUTTON', 'Option', 3, 130, 103)
user.ShowWindow(window, 4)
message = W.MSG()
while user.GetMessageW(C.byref(message), None, 0, 0) > 0:
    user.TranslateMessage(C.byref(message))
    user.DispatchMessageW(C.byref(message))
