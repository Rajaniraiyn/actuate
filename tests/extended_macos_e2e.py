"""Additional native controls, snapshot diffs, and screenshot-coordinate checks."""
import subprocess
import json
import tempfile
import time
from pathlib import Path
from macos_e2e import Session, value
from skylight_e2e import wait_for, cursor_position


def main():
    fixture = subprocess.Popen(['native/macos/.build/fixture'])
    session = Session()
    try:
        tree = None
        def observe():
            return session.result(op='observe', request={'pid': fixture.pid})
        def loaded():
            nonlocal tree
            tree = observe()
            return any(value(n, 'AXIdentifier') == 'actuate-test-checkbox' for n in tree['nodes'])
        wait_for(loaded, 'Expanded fixture not available')
        nodes = {value(n, 'AXIdentifier'): n for n in tree['nodes']}
        ref = lambda name: nodes[name]['reference']
        field = ref('actuate-test-field')
        def field_value():
            return session.result(op='attribute', target=field, name='AXValue')['value']
        def press(name, mode='skylight'):
            return session.result(op='click', target=ref(name), mode=mode)
        session.result(op='semantic', target=tree['root'],
                       action={'kind': 'set_bool', 'attribute': 'AXFrontmost', 'value': True})
        press('actuate-test-checkbox', 'semantic')
        wait_for(lambda: field_value() == 'checked', 'Checkbox did not turn on')
        press('actuate-test-checkbox', 'semantic')
        wait_for(lambda: field_value() == 'unchecked', 'Checkbox did not turn off')
        press('actuate-test-slider')
        wait_for(lambda: str(field_value()).startswith('slider:'), 'Slider click not consumed')
        assert field_value() != 'slider:20'
        checkbox=nodes['actuate-test-checkbox']; p=checkbox['attributes']['AXPosition']; z=checkbox['attributes']['AXSize']
        session.result(op='skylight_pointer',target=checkbox['reference'],action={'kind':'click','point':{'x':p['x']+8,'y':p['y']+z['height']/2}})
        wait_for(lambda: field_value() == 'checked', 'Checkbox glyph click not consumed')
        print('PASS checkbox semantic toggles, explicit glyph click, and native slider click')

        before = observe()
        press('actuate-add-item', 'semantic')
        after = observe()
        added = session.result(op='diff', before=before['revision'], after=after['revision'])
        dynamic = next(n for n in added['newly_observed'] if value(n, 'AXIdentifier') == 'actuate-dynamic-item')
        press('actuate-remove-item', 'semantic')
        removed_tree = observe()
        removed = session.result(op='diff', before=after['revision'], after=removed_tree['revision'])
        assert any(n['reference'] == dynamic['reference'] for n in removed['removed_from_scope']), removed
        after_nodes = {value(n, 'AXIdentifier'): n for n in removed_tree['nodes']}
        assert after_nodes['actuate-test-button']['reference'] == ref('actuate-test-button')
        with tempfile.TemporaryDirectory(prefix='actuate-offline-') as directory:
            before_file=Path(directory)/'before.json'; after_file=Path(directory)/'after.json'
            before_file.write_text(json.dumps(before)); after_file.write_text(json.dumps(after))
            def cli(*args):
                return json.loads(subprocess.run(['target/debug/actuate',*args],check=True,text=True,capture_output=True).stdout)
            assert cli('diff',str(before_file),str(after_file)) == added
            only=cli('diff','--modified-only',str(before_file),str(after_file))
            assert only['modified']==added['modified'] and 'newly_observed' not in only
            matches=cli('query','--role','AXButton',str(after_file))['matches']
            assert matches and all(value(n,'AXRole')=='AXButton' for n in matches)
        print('PASS insertion/removal diffs, unchanged sibling reference, and offline CLI diff/query')

        press('actuate-test-popup', 'semantic')
        menu = None
        def menu_open():
            nonlocal menu
            menu = observe()
            return any(value(n, 'AXRole') == 'AXMenuItem' and value(n, 'AXTitle') == 'Beta' for n in menu['nodes'])
        wait_for(menu_open, 'Popup menu items not observed')
        beta = next(n for n in menu['nodes'] if value(n, 'AXRole') == 'AXMenuItem' and value(n, 'AXTitle') == 'Beta')
        session.result(op='semantic', target=beta['reference'], action={'kind': 'perform', 'name': 'AXPress'})
        wait_for(lambda: field_value() == 'Beta', 'Popup selection not consumed')
        print('PASS native popup menu query and selection')

        press('actuate-test-dialog', 'semantic')
        modal = None
        def modal_open():
            nonlocal modal
            modal = observe()
            return any(value(n, 'AXRole') == 'AXSheet' for n in modal['nodes'])
        wait_for(modal_open, 'Modal sheet not observed')
        blocked=session.call(op='click',target=ref('actuate-test-button'),mode='global')
        assert blocked.get('error',{}).get('code')=='blocked_by_modal', blocked
        assert blocked['error']['effect']=='none', blocked
        dismiss = next(n for n in modal['nodes'] if value(n, 'AXRole') == 'AXButton' and value(n, 'AXTitle') == 'Dismiss')
        session.result(op='click', target=dismiss['reference'], mode='semantic')
        wait_for(lambda: field_value() == 'dismissed', 'Modal sheet not dismissed')
        print('PASS modal sheet query and dismissal')

        button = next(n for n in observe()['nodes'] if value(n, 'AXIdentifier') == 'actuate-test-button')
        position, size = button['attributes']['AXPosition'], button['attributes']['AXSize']
        center = {'x': position['x']+size['width']/2, 'y': position['y']+size['height']/2}
        window = session.result(op='window', target=button['reference'])
        local = {'x': center['x']-window['bounds']['x'], 'y': center['y']-window['bounds']['y']}
        session.result(op='click_window', target=button['reference'], point=local, mode='skylight')
        wait_for(lambda: field_value() == 'clicked', 'Window-local click not consumed')
        print('PASS window-local coordinates')
        before_cursor=cursor_position()
        before_active=session.result(op='discover')['active_pid']
        session.result(op='cursor_overlay',action={'kind':'start','executable':str(Path('target/debug/actuate').resolve())})
        try:
            session.result(op='click',target=button['reference'],mode='skylight')
            state=session.result(op='cursor_state')
            assert state['skylight']['pid']==fixture.pid, state
            assert state['skylight']['window_id']==window['window_id'], state
            assert abs(state['skylight']['desktop']['x']-center['x']) < .01, state
            assert abs(state['skylight']['desktop']['y']-center['y']) < .01, state
            assert cursor_position()==before_cursor
            assert session.result(op='discover')['active_pid']==before_active
            time.sleep(.3)
            display=session.result(op='displays')[0]['display_id']
            Path('/tmp/overlay-validation.png').unlink(missing_ok=True)
            Path('/tmp/overlay-executable.png').unlink(missing_ok=True)
            session.result(op='capture',source={'kind':'display','display_id':display},path='/tmp/overlay-validation.png')
            subprocess.run(['/usr/sbin/screencapture','-x','/tmp/overlay-executable.png'],check=True)
            print('OVERLAY_STATE',session.result(op='cursor_state'),flush=True)
            print('PASS overlay launch with targeted input, virtual pointer state, shared cursor/focus unchanged')
        finally:
            session.result(op='cursor_overlay',action={'kind':'stop'})

        with tempfile.TemporaryDirectory(prefix='actuate-capture-test-') as directory:
            session.result(op='semantic',target=field,action={'kind':'set_string','attribute':'AXValue','value':'image-reset'})
            frame = session.result(op='capture', source={'kind':'window','window_id':window['window_id']},
                                   path=str(Path(directory)/'window.png'), max_pixel_edge=300)
            mapping=frame['frame']['mapping']; bounds=mapping['source_bounds']
            assert max(mapping['pixel_width'],mapping['pixel_height']) == 300
            pixels={'x':(center['x']-bounds['x'])*mapping['pixel_width']/bounds['width'],
                    'y':(center['y']-bounds['y'])*mapping['pixel_height']/bounds['height']}
            session.result(op='click_image',frame=frame['frame_id'],point=pixels,mode='skylight')
            wait_for(lambda: field_value() == 'clicked', 'Resized screenshot click not consumed')
            stale=session.call(op='click_image',frame=frame['frame_id'],point=pixels,mode='skylight')
            assert stale['error']['code']=='stale_frame', stale
            assert stale['error']['effect']=='none', stale
            print('PASS resized native screenshot click mapping and stale-frame rejection')
    finally:
        fixture.terminate()
        fixture.wait(timeout=5)
        session.close()


if __name__ == '__main__':
    main()
