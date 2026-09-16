"""Visible native-binary drag acceptance using disposable Explorer files."""
import argparse
import hashlib
import json
import subprocess
import time
import uuid
import ctypes as C
from ctypes import wintypes as W
from pathlib import Path
from windows_interactive import ROOT, Session, Overlay, USER, DWM, class_name, owner, title, eventually, desktop_state


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--interactive',action='store_true')
    parser.add_argument('--lead-in',type=float,default=5,help='Seconds before opening the disposable Explorer windows')
    parser.add_argument('--clean-background',action='store_true',help='Explicitly minimize ordinary windows on the current workspace for recording')
    parser.add_argument('--output',type=Path,default=ROOT/'target/windows-interactive/explorer-drag')
    args=parser.parse_args()
    if not args.interactive:
        parser.error('--interactive requires an available desktop')
    args.output.mkdir(parents=True,exist_ok=True)
    root=(args.output/('files-'+uuid.uuid4().hex[:8])).resolve()
    source=root/'Actuate source'
    destination=root/'Actuate destination'
    source.mkdir(parents=True); destination.mkdir()
    data=b'Actuate disposable drag acceptance.\r\n'
    (source/'Drag sample.txt').write_bytes(data)
    folder=source/'Drag folder'; folder.mkdir()
    (folder/'nested.txt').write_bytes(data+b'nested\r\n')
    session=Session(args.output)
    overlay=Overlay(args.output,session,'hide_while_visible','physical_pointer')
    windows=[]
    result={'status':'running','root':str(root)}
    previous=desktop_state()['cursor']
    def snapshot(hwnd):
        return session.call('snapshot',window_id=hwnd,request=dict(pid=owner(hwnd),max_nodes=800,max_depth=30))
    def key(code,**mods):
        return session.call('key',delivery=dict(kind='global'),chord=dict(key_code=code,modifiers=mods))
    def capture(name):
        session.call('capture',path=str((args.output/(name+'.png')).resolve()))
    def pointer(action):
        deadline=time.monotonic()+20
        while True:
            try:
                return session.call('pointer',delivery=dict(kind='global'),action=action)
            except AssertionError as error:
                reply=error.args[0] if error.args else None
                if not isinstance(reply,dict) or reply.get('error',{}).get('code')!='shared_input_busy' or time.monotonic()>=deadline:
                    raise
                print('Waiting for held recording keys or mouse buttons to be released',flush=True)
                time.sleep(1)
    try:
        if args.clean_background:
            minimized=[]
            for window in session.call('windows'):
                hwnd=window['window_id']
                cloaked=W.DWORD()
                DWM.DwmGetWindowAttribute(hwnd,14,C.byref(cloaked),4)
                if window['visible'] and not cloaked.value and class_name(hwnd) in ('CASCADIA_HOSTING_WINDOW_CLASS','Chrome_WidgetWin_1','CabinetWClass'):
                    USER.ShowWindow(hwnd,6)
                    minimized.append(hwnd)
            result['minimized_for_recording']=minimized
        print(f'Starting visible drag test in {args.lead_in:g} seconds',flush=True)
        time.sleep(max(0,min(args.lead_in,30)))
        for path,x in [(source,20),(destination,980)]:
            subprocess.Popen(['explorer.exe','/n,',str(path)],creationflags=subprocess.CREATE_NO_WINDOW)
            hwnd=eventually(lambda:next((w['window_id'] for w in session.call('windows') if str(path) in w['title'] or w['title']==path.name),None),20)
            windows.append(hwnd)
            USER.ShowWindow(hwnd,9)
            USER.SetWindowPos(hwnd,0,x,100,900,850,0x10)
            time.sleep(1)
        src,dst=windows
        overlay.command('scope',scope=dict(kind='desktop'))
        overlay.command('show')
        evidence=[]
        for name,expected in [('Drag sample.txt',data),('Drag folder',data+b'nested\r\n')]:
            USER.SetForegroundWindow(src)
            tree=snapshot(src)
            (args.output/('source-'+name.replace('.','_')+'.json')).write_text(json.dumps(tree,indent=2),encoding='utf-8')
            matches=[n for n in tree['nodes'] if n['attributes'].get('name') in (name,Path(name).stem) and n['attributes'].get('control_type')==50007]
            assert len(matches)==1,[(n['attributes'].get('name'),n['attributes'].get('control_type')) for n in tree['nodes']]
            inspected=session.call('inspect',target=matches[0]['reference'])
            point=inspected['attributes'].get('clickable_point')
            child_ids={c['id'] for c in matches[0]['children']}
            label=next((n for n in tree['nodes'] if n['reference']['id'] in child_ids and n['attributes'].get('value')==name and not n['attributes'].get('offscreen')),None)
            if label or not point:
                # Explorer rows can extend beyond their clipped viewport.
                # Keep the fallback inside both the row and the Items View.
                row=(label or inspected)['attributes']['bounds']
                view=next(n['attributes']['bounds'] for n in tree['nodes'] if n['attributes'].get('name')=='Items View')
                left=max(row['x'],view['x']); top=max(row['y'],view['y'])
                right=min(row['x']+row['width'],view['x']+view['width'])
                bottom=min(row['y']+row['height'],view['y']+view['height'])
                assert right>left and bottom>top
                # The Name cell includes trailing whitespace that Explorer can
                # treat as marquee-selection space. Start within the text inset.
                point=dict(x=left+min(20,(right-left)/2),y=(top+bottom)/2)
            start=dict(x=point['x'],y=point['y'])
            session.call('semantic',target=matches[0]['reference'],action=dict(kind='perform',name='select'))
            tree=snapshot(dst)
            (args.output/'destination-tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
            view=next(n for n in tree['nodes'] if n['attributes'].get('name')=='Items View')
            b=view['attributes']['bounds']
            end=dict(x=b['x']+b['width']*.7,y=b['y']+b['height']*.7)
            pointer(dict(kind='move',point=start))
            time.sleep(1)
            capture('before-'+name.replace('.','_'))
            print('Dragging '+name+' using native SendInput',flush=True)
            receipt=pointer({'kind':'drag','from':start,'to':end,'button':'left','modifiers':{},'duration_ms':2400})
            copied=destination/name if name.endswith('.txt') else destination/name/'nested.txt'
            eventually(lambda:copied.exists(),15)
            assert copied.read_bytes()==expected
            assert not (source/name).exists()
            evidence.append(dict(name=name,receipt=receipt,sha256=hashlib.sha256(copied.read_bytes()).hexdigest(),source_removed=True))
            capture('after-'+name.replace('.','_'))
            time.sleep(2)
        result.update(status='pass',checks=evidence)
        print('PASS file and folder drag between Explorer windows',flush=True)
        time.sleep(8)
    except Exception as error:
        result.update(status='fail',error=str(error))
        capture('failure')
        raise
    finally:
        overlay.close()
        for hwnd in windows:
            USER.PostMessageW(hwnd,0x10,0,0)
        try:
            session.call('pointer',delivery=dict(kind='global'),action=dict(kind='move',point=dict(x=previous[0],y=previous[1])))
        except AssertionError as error:
            result['pointer_restore_error']=str(error)
        session.close()
        (args.output/'results.json').write_text(json.dumps(result,indent=2),encoding='utf-8')


if __name__=='__main__':
    main()
