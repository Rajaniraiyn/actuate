"""Opt-in acceptance for native Rust Save/Open dialogs and modal prompts."""
import argparse
import json
import subprocess
import time
import traceback
import uuid
import sys
from pathlib import Path
from windows_interactive import ROOT, Session, Overlay, USER, owner, eventually
from windows_e2e import isolated_parent, assert_isolated

PAYLOAD=b'native Rust save fixture\r\n'


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--interactive',action='store_true')
    parser.add_argument('--isolated',action='store_true',help='Use UIA on a private inactive Win32 desktop')
    parser.add_argument('--isolated-worker',action='store_true',help=argparse.SUPPRESS)
    parser.add_argument('--output',type=Path,default=ROOT/'target/windows-interactive/dialogs')
    args=parser.parse_args()
    if args.interactive == args.isolated:
        parser.error('Choose --interactive or --isolated')
    args.output.mkdir(parents=True,exist_ok=True)
    if args.isolated:
        log=args.output/'worker.log'
        if not args.isolated_worker:
            raise SystemExit(isolated_parent(__file__,log))
        sys.stdout=sys.stderr=log.open('w',encoding='utf-8',buffering=1)
        assert_isolated()
    root=(args.output/('files-'+uuid.uuid4().hex[:8])).resolve()
    root.mkdir()
    session=Session(args.output)
    overlay=Overlay(args.output,session,'hide_while_visible') if args.interactive else None
    if overlay:
        overlay.command('scope',scope=dict(kind='desktop'))
    checks=[]
    processes=[]

    def snapshot(hwnd):
        return session.call('snapshot',window_id=hwnd,request=dict(pid=owner(hwnd),max_nodes=700,max_depth=30))

    def capture(name):
        if args.interactive:
            session.call('capture',path=str((args.output/(name+'.png')).resolve()))

    def start(mode):
        process=subprocess.Popen([str(ROOT/'target/debug/examples/windows_dialog_fixture.exe'),'--interactive',mode,str(root)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,encoding='utf-8',creationflags=subprocess.CREATE_NO_WINDOW)
        processes.append(process)
        title='Unimation native Save' if mode=='save' else 'Unimation native Open'
        def ready():
            if process.poll() is not None:
                out,err=process.communicate()
                raise RuntimeError(f'Native fixture exited {process.returncode}: {out} {err}')
            return USER.FindWindowW(None,title)
        hwnd=eventually(ready,15)
        assert owner(hwnd)==process.pid
        if args.interactive:
            USER.SetForegroundWindow(hwnd)
        return process,hwnd

    def control(tree,name,action,control_type=None,identifier=None):
        candidates=[n for n in tree['nodes'] if n['attributes'].get('name','').replace('&','').rstrip(':')==name and action in n['actions'] and (control_type is None or n['attributes'].get('control_type')==control_type) and (identifier is None or n['attributes'].get('automation_id')==identifier)]
        assert len(candidates)==1,(name,[(n['attributes'].get('name'),n['attributes'].get('automation_id'),n['actions']) for n in tree['nodes'] if n['actions']])
        return candidates[0]

    def filename(hwnd,value):
        def ready():
            tree=snapshot(hwnd)
            return tree if any(n['attributes'].get('name','').replace('&','').rstrip(':')=='File name' and 'set_value' in n['actions'] for n in tree['nodes']) else None
        tree=eventually(ready,15)
        editor=control(tree,'File name','set_value',control_type=50004)
        session.call('semantic',target=editor['reference'],action=dict(kind='set_string',attribute='value',value=value))
        assert session.call('inspect',target=editor['reference'])['attributes']['value']==value
        return tree

    def click(node):
        if args.isolated:
            reply=session.call('semantic',target=node['reference'],action=dict(kind='perform',name='invoke'),allow_error=True)
            # Modal providers can time out after opening a prompt. Never retry
            # an uncertain action; the caller verifies the resulting UI/state.
            if 'error' in reply:
                assert reply['error']['effect']=='unknown',reply
            return
        b=node['attributes']['bounds']
        x,y=b['x']+b['width']/2,b['y']+b['height']/2
        overlay.move(x,y,400); time.sleep(.5)
        overlay.command('click',x=x,y=y)
        session.call('pointer',delivery=dict(kind='global'),action=dict(kind='click',point=dict(x=x,y=y),button='left',count=1,modifiers={}))

    def finish(process,status):
        out,err=process.communicate(timeout=15)
        assert process.returncode==0,(out,err)
        result=json.loads(out)
        assert result['status']==status,result
        return result

    def prompt(process,original,name):
        def ready():
            for w in session.call('windows'):
                if w['pid']==process.pid and w['visible'] and w['window_id']!=original:
                    tree=snapshot(w['window_id'])
                    if any(n['attributes'].get('name','').replace('&','')==name for n in tree['nodes']):
                        return w['window_id'],tree
        return eventually(ready,10)

    def check(name,operation):
        print('Testing '+name,flush=True)
        try:
            evidence=operation()
            checks.append(dict(name=name,status='pass',evidence=evidence))
            print('PASS '+name,flush=True)
        except Exception as error:
            checks.append(dict(name=name,status='fail',error=str(error),traceback=traceback.format_exc()))
            print('FAIL '+name+': '+str(error),flush=True)
            capture(name+'-failure')
        finally:
            if overlay:
                overlay.command('hide')
            # Close only the disposable fixture processes from this check.
            for process in processes:
                if process.poll() is None:
                    process.terminate(); process.wait(timeout=5)

    def unicode_save_open():
        name='report caf\u00e9 \U0001f980.txt'
        process,hwnd=start('save')
        tree=filename(hwnd,name)
        (args.output/'save-tree.json').write_text(json.dumps(tree,indent=2),encoding='utf-8')
        capture('unicode-save')
        save_button=control(snapshot(hwnd),'Save','invoke')
        click(save_button)
        saved=finish(process,'saved')
        assert (root/name).read_bytes()==PAYLOAD
        stale=session.call('inspect',target=save_button['reference'],allow_error=True)
        assert 'error' in stale and stale['error']['effect']=='none',stale
        process,hwnd=start('open')
        filename(hwnd,name)
        click(control(snapshot(hwnd),'Open','invoke',identifier='1'))
        opened=finish(process,'opened')
        assert bytes(opened['bytes'])==PAYLOAD
        return dict(saved=saved,opened=opened,closed_dialog_reference_rejected=True)

    def overwrite(accept):
        path=root/('replace.txt' if accept else 'keep.txt')
        path.write_bytes(b'original fixture contents')
        process,hwnd=start('save')
        filename(hwnd,path.name)
        click(control(snapshot(hwnd),'Save','invoke'))
        modal,tree=prompt(process,hwnd,'Yes')
        (args.output/('overwrite-'+str(accept)+'-tree.json')).write_text(json.dumps(tree,indent=2),encoding='utf-8')
        assert path.read_bytes()==b'original fixture contents'
        capture('overwrite-'+str(accept))
        click(control(tree,'Yes' if accept else 'No','invoke'))
        if accept:
            result=finish(process,'saved')
            assert path.read_bytes()==PAYLOAD
        else:
            eventually(lambda:not USER.IsWindowVisible(modal))
            click(control(snapshot(hwnd),'Cancel','invoke'))
            result=finish(process,'cancelled')
            assert path.read_bytes()==b'original fixture contents'
        return result

    def rejected_name(mode,value):
        before=set(root.iterdir())
        process,hwnd=start(mode)
        filename(hwnd,value)
        click(control(snapshot(hwnd),'Save' if mode=='save' else 'Open','invoke',identifier='1'))
        modal,tree=prompt(process,hwnd,'OK')
        capture('rejected-'+mode)
        assert set(root.iterdir())==before
        click(control(tree,'OK','invoke'))
        eventually(lambda:not USER.IsWindowVisible(modal))
        click(control(snapshot(hwnd),'Cancel','invoke'))
        result=finish(process,'cancelled')
        assert set(root.iterdir())==before
        return result

    def nested_default_extension():
        folder=root/'nested folder'
        folder.mkdir()
        process,hwnd=start('save')
        filename(hwnd,'nested folder\\report caf\u00e9')
        click(control(snapshot(hwnd),'Save','invoke'))
        result=finish(process,'saved')
        assert (folder/'report caf\u00e9.txt').read_bytes()==PAYLOAD
        return result

    try:
        check('unicode_save_and_open',unicode_save_open)
        check('overwrite_decline_then_cancel',lambda:overwrite(False))
        check('overwrite_accept',lambda:overwrite(True))
        check('invalid_filename_then_cancel',lambda:rejected_name('save','bad<name>.txt'))
        check('nested_path_and_default_extension',nested_default_extension)
        check('missing_open_file_then_cancel',lambda:rejected_name('open','does-not-exist.txt'))
    finally:
        if overlay:
            overlay.close()
        session.close()
        (args.output/'results.json').write_text(json.dumps(dict(root=str(root),checks=checks),indent=2),encoding='utf-8')
    raise SystemExit(int(any(c['status']=='fail' for c in checks)))


if __name__=='__main__':
    main()
