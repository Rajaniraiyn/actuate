"""Opt-in visible Windows acceptance tests. Requires an available interactive session."""
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import subprocess
import time
import traceback
import sys
import os
from types import SimpleNamespace
from windows_e2e import ROOT, Session, USER, desktop_state, eventually, rect
sys.path.insert(0,str(ROOT/'target/windows-interactive/python-deps'))

USER.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
USER.FindWindowW.argtypes = [W.LPCWSTR, W.LPCWSTR]
USER.FindWindowW.restype = W.HWND
USER.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
USER.GetWindowTextW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
USER.SendMessageTimeoutW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM, W.UINT, W.UINT, C.POINTER(C.c_size_t)]
USER.SendMessageTimeoutW.restype = C.c_ssize_t
USER.SetForegroundWindow.argtypes = [W.HWND]
DWM = C.WinDLL('dwmapi')
DWM.DwmGetWindowAttribute.argtypes = [W.HWND, W.DWORD, W.LPVOID, W.DWORD]


def owner(hwnd):
    pid = W.DWORD()
    USER.GetWindowThreadProcessId(hwnd, C.byref(pid))
    return pid.value


def class_name(hwnd):
    text = C.create_unicode_buffer(256)
    USER.GetClassNameW(hwnd, text, len(text))
    return text.value


def title(hwnd):
    text=C.create_unicode_buffer(512)
    USER.GetWindowTextW(hwnd,text,len(text))
    return text.value


def frame(hwnd):
    value = W.RECT()
    if DWM.DwmGetWindowAttribute(hwnd, 9, C.byref(value), C.sizeof(value)) == 0:
        return [value.left, value.top, value.right, value.bottom]
    return rect(hwnd)


class Overlay:
    def __init__(self, output, session, physical_cursor='preserve',tracking='commands'):
        self.session=session
        state=session.call('cursor_overlay',action=dict(kind='start',executable=str(ROOT/'target/debug/unimation-overlay.exe'),physical_cursor=physical_cursor,tracking=tracking))
        self.process=SimpleNamespace(pid=state['pid'])
        self.command('configure', appearance={'color': [.62,.3,.96], 'scale':1.3, 'idle': {'style':'off'}})

    def command(self, op, **fields):
        return self.session.call('cursor',command=dict(op=op,**fields))

    def scope(self, hwnd):
        self.command('scope', scope=dict(kind='window', window_id=hwnd, pid=owner(hwnd)))

    def move(self, x, y, duration=600):
        self.command('move', x=x, y=y, duration_ms=duration)

    def close(self):
        self.session.call('cursor_overlay',action=dict(kind='stop'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--interactive', action='store_true', help='Confirm this session is available for visible tests')
    parser.add_argument('--phase', choices=['overlay','shell'], default='overlay')
    parser.add_argument('--virtual-desktops',type=Path,help='Optional explicitly supplied VirtualDesktopAccessor DLL for workspace acceptance')
    parser.add_argument('--physical-cursor',choices=['preserve','hide_within_scope','hide_while_visible'],default='preserve')
    parser.add_argument('--case', choices=['run','quick','start','calculator','settings','store','control','taskbar','tray'], default='run')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/windows-interactive/overlay')
    args = parser.parse_args()
    if not args.interactive:
        parser.error('Visible tests require --interactive and an available session')
    args.output.mkdir(parents=True, exist_ok=True)
    session = Session(args.output)
    overlay = Overlay(args.output,session,args.physical_cursor)
    fixtures = []
    results = {'checks': [], 'initial_desktop': desktop_state()}
    def check(name, operation):
        print('Testing '+name, flush=True)
        try:
            evidence = operation()
            results['checks'].append(dict(name=name,status='pass',evidence=evidence))
            print('PASS '+name, flush=True)
        except Exception as error:
            results['checks'].append(dict(name=name,status='fail',error=str(error),traceback=traceback.format_exc()))
            print('FAIL '+name+': '+str(error), flush=True)
    def capture(name):
        return session.call('capture',path=str((args.output/(name+'.png')).resolve()))
    def snapshot(hwnd):
        return session.call('snapshot',window_id=hwnd,request=dict(pid=owner(hwnd),max_nodes=800,max_depth=30))
    def semantic(node, name):
        bounds=node['attributes'].get('bounds')
        if bounds:
            x=bounds['x']+bounds['width']/2; y=bounds['y']+bounds['height']/2
            overlay.move(x,y,450); time.sleep(.6)
            overlay.command('click',x=x,y=y)
        return session.call('semantic',target=node['reference'],action=dict(kind='perform',name=name))
    def key(code, **modifiers):
        return session.call('key',delivery=dict(kind='global'),chord=dict(key_code=code,modifiers=modifiers))
    def pointer(action):
        return session.call('pointer',delivery=dict(kind='global'),action=action)
    try:
        results['initial_windows'] = session.call('windows')
        if args.phase == 'overlay':
            log = (args.output/'fixture.stderr').open('w',encoding='utf-8')
            fixture = subprocess.Popen(['powershell.exe','-NoProfile','-STA','-ExecutionPolicy','Bypass','-File',str(ROOT/'tests/windows_fixture.ps1'),'-Toolkit','wpf','-Interactive'], stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW)
            fixtures.append((fixture,log))
            target = eventually(lambda: next((w for w in session.call('windows') if w['pid']==fixture.pid and w['title']=='Unimation WPF fixture' and w['visible']),None),20)
            hwnd = target['window_id']
            USER.SetForegroundWindow(hwnd)
            time.sleep(.5)
            def presentation():
                before = desktop_state()
                bounds = frame(hwnd)
                overlay.scope(hwnd)
                overlay.move(bounds[0]+100,bounds[1]+100,0)
                time.sleep(1)
                windows = [w for w in session.call('windows') if w['pid']==overlay.process.pid]
                capture('initial-overlay')
                assert any(w['visible'] for w in windows), windows
                oh = next(w['window_id'] for w in windows if w['visible'])
                start = rect(oh)
                overlay.move(bounds[0]+300,bounds[1]+100,800)
                samples = []
                for _ in range(25):
                    samples.append(rect(oh)); time.sleep(.04)
                assert len(set(r[0] for r in samples))>4, samples
                assert abs(rect(oh)[0]-start[0]-200)<=2
                unchanged=desktop_state()==before
                capture('animated-overlay')
                time.sleep(3)
                return dict(overlay=oh,samples=samples,desktop_before=before,desktop_after=desktop_state(),foreground_and_pointer_unchanged=unchanged)
            check('visible_soft_cursor_and_animation',presentation)
            def attachment():
                from PIL import Image
                USER.SetForegroundWindow(hwnd); time.sleep(.4)
                oh=next(w['window_id'] for w in session.call('windows') if w['pid']==overlay.process.pid and w['title']=='Unimation cursor')
                original=rect(hwnd); bounds=frame(hwnd)
                before=desktop_state()
                overlay.move(bounds[0]+200,bounds[1]+180,500); time.sleep(.8)
                cursor=rect(oh)
                assert USER.SetWindowPos(hwnd,None,original[0]+130,original[1]+90,0,0,0x15)
                eventually(lambda: abs(rect(oh)[0]-cursor[0]-130)<=1)
                assert abs(rect(oh)[1]-cursor[1]-90)<=1
                capture('window-moved')
                cover=next(w['window_id'] for w in session.call('windows') if w['pid']==fixture.pid and w['title']=='Unimation occlusion test')
                cr=rect(oh)
                assert USER.SetWindowPos(cover,0,cr[0]-40,cr[1]-40,360,220,0x10)
                time.sleep(.6)
                assert USER.GetWindow(oh,2)==hwnd
                capture('occluded')
                def purple(path, region):
                    image=Image.open(args.output/(path+'.png')).convert('RGB').crop(tuple(region))
                    return sum(120<r<195 and 35<g<105 and b>205 for r,g,b in image.get_flattened_data())
                assert purple('window-moved',cr)>40
                assert purple('occluded',cr)==0
                USER.ShowWindow(cover,0)
                time.sleep(.4)
                bounds=frame(hwnd)
                overlay.move(bounds[2]-4,bounds[1]+200,500); time.sleep(.8)
                capture('edge-clipped')
                cr=rect(oh)
                assert purple('edge-clipped',[bounds[2],cr[1],cr[2],cr[3]])==0
                USER.ShowWindow(hwnd,6)
                eventually(lambda:not USER.IsWindowVisible(oh))
                USER.ShowWindow(hwnd,4)
                eventually(lambda:USER.IsWindowVisible(oh))
                overlay.command('scope',scope=dict(kind='window',window_id=hwnd,pid=fixture.pid+1))
                eventually(lambda:not USER.IsWindowVisible(oh))
                overlay.scope(hwnd)
                eventually(lambda:USER.IsWindowVisible(oh))
                return dict(translates_with_target=True,occlusion_pixels_verified=True,edge_clipping_verified=True,minimize_and_wrong_owner_hide=True,pointer_unchanged=desktop_state()['cursor']==before['cursor'])
            check('window_attachment_occlusion_clipping_and_minimize',attachment)
            def clicks():
                USER.SetForegroundWindow(hwnd); time.sleep(.4)
                tree=snapshot(hwnd)
                button=next(n for n in tree['nodes'] if n['attributes'].get('automation_id')=='increment')
                old=desktop_state()['cursor']
                semantic(button,'invoke')
                eventually(lambda:any(n['attributes'].get('name')=='count:1' for n in snapshot(hwnd)['nodes']))
                assert desktop_state()['cursor']==old
                b=button['attributes']['bounds']; x=b['x']+b['width']/2; y=b['y']+b['height']/2
                pointer(dict(kind='click',point=dict(x=x,y=y),button='left',count=1,modifiers={}))
                eventually(lambda:any(n['attributes'].get('name')=='count:2' for n in snapshot(hwnd)['nodes']))
                assert USER.GetForegroundWindow()==hwnd
                capture('click-through')
                pointer(dict(kind='move',point=dict(x=old[0],y=old[1])))
                time.sleep(2)
                return dict(semantic_changed_counter_without_pointer_movement=True,physical_click_passed_through_overlay=True)
            check('semantic_cursor_and_physical_click_through',clicks)
            def decoration():
                USER.SetForegroundWindow(hwnd)
                bounds=frame(hwnd)
                x,y=bounds[0]+250,bounds[1]+230
                overlay.command('configure',appearance=dict(color=[.62,.3,.96],scale=1.3,idle=dict(style='wave',amplitude=1.2,period_ms=2400)))
                overlay.move(x,y,700)
                time.sleep(1)
                overlay.command('click',x=x,y=y)
                time.sleep(.08)
                capture('shared-ripple')
                time.sleep(2)
                capture('idle-wave')
                time.sleep(3)
                overlay.command('configure',appearance=dict(color=[.62,.3,.96],scale=1.3,motion='reduced',idle=dict(style='wave')))
                overlay.move(x+150,y,2000)
                time.sleep(.3)
                capture('reduced-motion')
                return dict(shared_ripple_and_wave_demonstrated=True,reduced_motion_demonstrated=True,visual_effects='screenshots; not pixel asserted')
            check('shared_decoration',decoration)
            if args.virtual_desktops:
                def workspaces():
                    dll=C.CDLL(str(args.virtual_desktops.resolve()))
                    for name in ('GetCurrentDesktopNumber','GetDesktopCount','CreateDesktop'):
                        getattr(dll,name).argtypes=[]
                        getattr(dll,name).restype=C.c_int
                    dll.MoveWindowToDesktopNumber.argtypes=[W.HWND,C.c_int]
                    dll.GetWindowDesktopNumber.argtypes=[W.HWND]
                    dll.GoToDesktopNumber.argtypes=[C.c_int]
                    dll.RemoveDesktop.argtypes=[C.c_int,C.c_int]
                    original=dll.GetCurrentDesktopNumber()
                    count=dll.GetDesktopCount()
                    assert original>=0 and count>0
                    created=dll.CreateDesktop()
                    assert created>=count,(created,count)
                    print(f'Created test workspace {created}; original {original}',flush=True)
                    try:
                        overlay.scope(hwnd)
                        bounds=frame(hwnd)
                        overlay.move(bounds[0]+220,bounds[1]+180,0)
                        oh=next(w['window_id'] for w in session.call('windows') if w['pid']==overlay.process.pid and w['title']=='Unimation cursor')
                        assert dll.MoveWindowToDesktopNumber(hwnd,created)==1
                        eventually(lambda:not USER.IsWindowVisible(oh))
                        assert dll.GetCurrentDesktopNumber()==original
                        assert dll.GetWindowDesktopNumber(hwnd)==created
                        tree=snapshot(hwnd)
                        button=next(n for n in tree['nodes'] if n['attributes'].get('automation_id')=='increment')
                        before=next(n['attributes']['name'] for n in tree['nodes'] if n['attributes'].get('name','').startswith('count:'))
                        session.call('semantic',target=button['reference'],action=dict(kind='perform',name='invoke'))
                        expected='count:'+str(int(before.split(':')[1])+1)
                        eventually(lambda:any(n['attributes'].get('name')==expected for n in snapshot(hwnd)['nodes']))
                        assert dll.GetCurrentDesktopNumber()==original
                        assert not USER.IsWindowVisible(oh)
                        capture('inactive-workspace')
                        assert dll.GoToDesktopNumber(created)==1
                        eventually(lambda:dll.GetCurrentDesktopNumber()==created)
                        eventually(lambda:USER.IsWindowVisible(oh))
                        capture('owned-workspace')
                        time.sleep(3)
                        assert dll.GoToDesktopNumber(original)==1
                        eventually(lambda:dll.GetCurrentDesktopNumber()==original)
                        eventually(lambda:not USER.IsWindowVisible(oh))
                        assert dll.MoveWindowToDesktopNumber(hwnd,original)==1
                        eventually(lambda:USER.IsWindowVisible(oh))
                        return dict(off_workspace_semantic_counter_verified=True,overlay_hides_and_returns=True,original=original,created=created)
                    finally:
                        dll.GoToDesktopNumber(original)
                        dll.MoveWindowToDesktopNumber(hwnd,original)
                        # Remove only our last-created workspace, and only when
                        # no discovered foreign window has moved into it.
                        foreign=[w for w in session.call('windows') if w['pid'] not in (fixture.pid,overlay.process.pid) and dll.GetWindowDesktopNumber(w['window_id'])==created]
                        if dll.GetDesktopCount()==count+1 and not foreign:
                            assert dll.RemoveDesktop(created,original)==1
                        else:
                            raise RuntimeError(f'Test workspace retained because desktop membership changed: {foreign}')
                check('virtual_desktop_attachment_and_background_semantics',workspaces)
        else:
            def shell_case():
                if args.case=='tray':
                    log=(args.output/'fixture.stderr').open('w',encoding='utf-8')
                    fixture=subprocess.Popen(['powershell.exe','-NoProfile','-STA','-ExecutionPolicy','Bypass','-File',str(ROOT/'tests/windows_fixture.ps1'),'-Toolkit','wpf','-Interactive'],stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW)
                    fixtures.append((fixture,log))
                    target=eventually(lambda:USER.FindWindowW(None,'Unimation WPF fixture'),20)
                    bar=session.call('taskbar_state')
                    old=desktop_state()['cursor']
                    try:
                        session.call('taskbar_auto_hide',enabled=False)
                        time.sleep(1)
                        tree=snapshot(bar['window_id'])
                        (args.output/'taskbar-tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
                        icons=[n for n in tree['nodes'] if 'Unimation test tray' in n['attributes'].get('name','')]
                        if not icons:
                            expand=next(n for n in tree['nodes'] if 'hidden icons' in n['attributes'].get('name','').lower() and 'invoke' in n['actions'])
                            semantic(expand,'invoke')
                            time.sleep(.7)
                            popup=eventually(lambda:USER.FindWindowW('TopLevelWindowForOverflowXamlIsland',None))
                            tree=snapshot(popup)
                            (args.output/'overflow-tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
                            icons=[n for n in tree['nodes'] if 'Unimation test tray' in n['attributes'].get('name','')]
                        assert len(icons)==1,[(n['attributes'].get('name'),n['actions']) for n in tree['nodes']]
                        b=icons[0]['attributes']['bounds']
                        x,y=b['x']+b['width']/2,b['y']+b['height']/2
                        overlay.command('scope',scope=dict(kind='desktop'))
                        overlay.move(x,y,600); time.sleep(.8)
                        pointer(dict(kind='click',point=dict(x=x,y=y),button='right',count=1,modifiers={}))
                        def find_menu():
                            for w in session.call('windows'):
                                if w['pid']==fixture.pid and w['visible']:
                                    tree=snapshot(w['window_id'])
                                    entries=[n for n in tree['nodes'] if n['attributes'].get('name')=='Increment test counter' and 'invoke' in n['actions']]
                                    if entries:
                                        return w['window_id'],entries[0],tree
                        try:
                            menu,item,tree=eventually(find_menu,3)
                        except AssertionError:
                            # Legacy ToolStrip can expose only its popup root to UIA.
                            menu=next(w['window_id'] for w in session.call('windows') if w['pid']==fixture.pid and w['visible'] and class_name(w['window_id']).startswith('WindowsForms10.Window'))
                            tree=snapshot(menu)
                            item=None
                        # Shell/tool popups may have no application-view workspace
                        # identity. Use explicit desktop presentation for this menu.
                        overlay.command('scope',scope=dict(kind='desktop'))
                        b=frame(menu)
                        overlay.move(b[0]+80,b[1]+12,450)
                        time.sleep(.7)
                        (args.output/'menu-tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
                        capture('tray-menu')
                        if item:
                            semantic(item,'invoke')
                        else:
                            key(0x28); key(0x0D)
                        eventually(lambda:any(n['attributes'].get('name')=='count:1' for n in snapshot(target)['nodes']))
                        capture('tray-counter')
                        return dict(owned_tray_menu_counter_verified=True,menu_hwnd=menu,route='uia' if item else 'native_keyboard',uia_menu_items_available=item is not None)
                    finally:
                        overlay.command('hide')
                        session.call('taskbar_auto_hide',enabled=bar['auto_hide'])
                        pointer(dict(kind='move',point=dict(x=old[0],y=old[1])))
                if args.case=='taskbar':
                    before=session.call('taskbar_state')
                    old=desktop_state()['cursor']
                    states={}
                    try:
                        assert not session.call('taskbar_auto_hide',enabled=False)['auto_hide']
                        time.sleep(1)
                        states['visible']=session.call('taskbar_state')
                        capture('taskbar-visible')
                        assert session.call('taskbar_auto_hide',enabled=True)['auto_hide']
                        pointer(dict(kind='move',point=dict(x=400,y=300)))
                        time.sleep(3)
                        states['hidden']=session.call('taskbar_state')
                        capture('taskbar-hidden')
                        overlay.command('scope',scope=dict(kind='desktop'))
                        b=states['visible']['bounds']
                        x=b['x']+b['width']/2
                        y=b['y']+b['height']-1
                        overlay.move(x,y,800)
                        time.sleep(2)
                        states['soft_cursor']=session.call('taskbar_state')
                        assert states['soft_cursor']['bounds']==states['hidden']['bounds'],states
                        capture('taskbar-soft-cursor')
                        pointer(dict(kind='move',point=dict(x=x,y=y)))
                        eventually(lambda:session.call('taskbar_state')['bounds']==states['visible']['bounds'],8)
                        capture('taskbar-physical-cursor')
                        return states
                    finally:
                        overlay.command('hide')
                        restored=session.call('taskbar_auto_hide',enabled=before['auto_hide'])
                        assert restored['flags']==before['flags'],(before,restored)
                        pointer(dict(kind='move',point=dict(x=old[0],y=old[1])))
                shortcuts={'run':(0x52,{'meta':True}),'quick':(0x41,{'meta':True}),'start':(0x5B,{})}
                old_hwnds={w['window_id'] for w in results['initial_windows'] if w['visible']}
                if args.case in shortcuts:
                    code,mods=shortcuts[args.case]
                    key(code,**mods)
                elif args.case in ('calculator','control'):
                    command=['calc.exe'] if args.case=='calculator' else ['control.exe','/name','Microsoft.Mouse']
                    subprocess.Popen(command,creationflags=subprocess.CREATE_NO_WINDOW)
                    wanted='Calculator' if args.case=='calculator' else 'Mouse Properties'
                    launched=eventually(lambda: USER.GetForegroundWindow() if wanted.lower() in title(USER.GetForegroundWindow()).lower() else None,20)
                elif args.case in ('settings','store'):
                    os.startfile('ms-settings:about' if args.case=='settings' else 'ms-windows-store://home')
                    wanted='Settings' if args.case=='settings' else 'Microsoft Store'
                    launched=eventually(lambda: USER.FindWindowW(None,wanted),25)
                else:
                    raise RuntimeError('Case not configured')
                time.sleep(.65)
                hwnd=USER.GetForegroundWindow() if args.case in shortcuts else launched
                try:
                    tree=snapshot(hwnd)
                    if args.case=='calculator':
                        def ready():
                            current=snapshot(hwnd)
                            return current if any(n['attributes'].get('automation_id')=='clearButton' for n in current['nodes']) else None
                        tree=eventually(ready,20)
                    overlay.scope(hwnd)
                    bounds=frame(hwnd)
                    overlay.move(bounds[0]+(bounds[2]-bounds[0])/2,bounds[1]+80,700)
                    time.sleep(.9)
                    capture(args.case+'-open')
                    (args.output/'tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
                    print(f"Observed {len(tree['nodes'])} nodes; traversal_complete={tree['traversal_complete']}",flush=True)
                    def node(name=None, identifier=None, action='invoke', source=None):
                        candidates=[n for n in (source or tree)['nodes'] if action in n['actions'] and (name is None or n['attributes'].get('name')==name) and (identifier is None or n['attributes'].get('automation_id')==identifier)]
                        assert len(candidates)==1,(name,identifier,len(candidates))
                        return candidates[0]
                    if args.case=='run':
                        editor=node(identifier='1001',action='set_value')
                        session.call('semantic',target=editor['reference'],action=dict(kind='set_string',attribute='value',value='winver'))
                        assert session.call('inspect',target=editor['reference'])['attributes']['value']=='winver'
                        capture('run-value')
                        semantic(node('Cancel'),'invoke')
                    elif args.case=='quick':
                        semantic(node('Manage Wi-Fi connections'),'invoke')
                        time.sleep(1)
                        next_tree=snapshot(hwnd)
                        (args.output/'wifi-tree.json').write_text(json.dumps(next_tree,indent=2),encoding='utf-8')
                        capture('wifi-details')
                        names=[n['attributes'].get('name','') for n in next_tree['nodes']]
                        assert any('Wi-Fi' in n for n in names),names
                        print(json.dumps([dict(name=n['attributes'].get('name'),actions=n['actions']) for n in next_tree['nodes'] if 'invoke' in n['actions']]),flush=True)
                    elif args.case=='calculator':
                        for identifier in ['clearButton','num2Button','plusButton','num3Button','equalButton']:
                            semantic(node(identifier=identifier),'invoke')
                            time.sleep(.35)
                        next_tree=snapshot(hwnd)
                        display=next(n['attributes']['name'] for n in next_tree['nodes'] if n['attributes'].get('automation_id')=='CalculatorResults')
                        assert display.strip().endswith('5'),display
                        capture('calculator-five')
                    elif args.case=='control':
                        hardware=node('Hardware',action='select')
                        semantic(hardware,'select')
                        assert session.call('inspect',target=hardware['reference'])['attributes']['selected']
                        capture('classic-hardware-tab')
                    elif args.case=='store':
                        apps=node(identifier='nav_productivity',action='select')
                        semantic(apps,'select')
                        eventually(lambda:session.call('inspect',target=apps['reference'])['attributes'].get('selected'))
                        capture('store-apps')
                    elif args.case=='settings':
                        home=node('Home',action='select')
                        semantic(home,'select')
                        eventually(lambda:session.call('inspect',target=home['reference'])['attributes'].get('selected'))
                        capture('settings-home')
                    time.sleep(3)
                    return dict(hwnd=hwnd,title=title(hwnd),window_class=class_name(hwnd),nodes=len(tree['nodes']),traversal_complete=tree['traversal_complete'])
                finally:
                    overlay.command('hide')
                    if args.case in ('calculator','settings','store','control') and hwnd not in old_hwnds:
                        USER.PostMessageW(hwnd,0x10,0,0)
                    elif USER.GetForegroundWindow()==hwnd:
                        key(0x1B)
            check(args.case,shell_case)
    finally:
        overlay.close()
        for process,log in fixtures:
            for w in session.call('windows'):
                if w['pid']==process.pid:
                    USER.PostMessageW(w['window_id'],0x10,0,0)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate(); process.wait()
            log.close()
        session.close()
        results['final_desktop']=desktop_state()
        (args.output/'results.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
    raise SystemExit(int(any(c['status']=='fail' for c in results['checks'])))


if __name__=='__main__':
    main()
