"""Host-specific iPad system input check. Use ONLY a disposable portrait simulator.
Lifecycle remains with the caller; this test does not create/delete devices.
"""
import argparse
from pathlib import Path
import tempfile
import time
from ios_simulator_e2e import Session, label, observe_until


def exercise(device_set, udid):
    session = Session(device_set, udid)
    try:
        session.result(op='button', button='home')
        time.sleep(1)
        # Explicit native AX hit-test scope, not an assumed display root. This
        # host's portrait iPad returns the home icon container at this AX point.
        home = session.result(op='observe', scope={'point': {'x':100.0, 'y':100.0}})
        settings = next(n for n in home['nodes'] if label(n) == 'Settings')
        session.result(op='semantic', target=settings['reference'], action={'kind':'perform','name':'AXPress'})
        settings_tree = observe_until(session, lambda t: any(label(n) == 'General' for n in t['nodes']))
        fields = [n for n in settings_tree['nodes'] if n['attributes'].get('AXRole',{}).get('value') == 'AXTextField']
        if fields:
            session.result(op='semantic', target=fields[0]['reference'], action={'kind':'perform','name':'AXPress'})
            session.result(op='hid_key', usage=5)
            observe_until(session, lambda t: any(str(n['attributes'].get('AXValue',{}).get('value','')).lower().endswith('b') for n in t['nodes']))
        else:
            raise AssertionError('Expected a visible Settings search field for HID verification')
        session.result(op='button',button='home')
        time.sleep(1)
        session.result(op='touch',action={'kind':'swipe','from':{'x':0.98,'y':0.001},'to':{'x':0.98,'y':0.55},'duration_ms':600,'edge':'top'})
        observe_until(session,lambda t:any(label(n)=='Add Controls' for n in t['nodes']))
        with tempfile.TemporaryDirectory(prefix='unimation-ios-capture-') as directory:
            capture=session.result(op='capture',path=str(Path(directory)/'control-center.png'))
            assert capture['pixel_width']>0 and capture['pixel_height']>0
            assert capture['click_mapping'] is None
        print('PASS iPad Home, explicit hit-test scope, semantic app launch, HID keyboard, Control Center edge swipe and capture',flush=True)
    finally:
        try:
            session.result(op='button',button='home')
        finally:
            session.close()

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--udid',required=True)
    parser.add_argument('--device-set',required=True)
    args=parser.parse_args()
    exercise(args.device_set,args.udid)
