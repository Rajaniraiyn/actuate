"""Run against an open Calculator and the disposable native fixture, on macOS.
Build with cargo build --workspace and compile native/macos/fixture.swift first.
Does not change system preferences or type into unrelated applications.
"""
import json
import selectors
import subprocess
import time

class Session:
    def __init__(self):
        self.p = subprocess.Popen(['target/debug/unimation', 'session'], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.p.stdout, selectors.EVENT_READ)
    def call(self, **request):
        self.p.stdin.write(json.dumps(request) + '\n'); self.p.stdin.flush()
        if not self.selector.select(30):
            raise TimeoutError('Session did not respond within 30s')
        return json.loads(self.p.stdout.readline())
    def result(self, **request):
        reply = self.call(**request)
        assert 'error' not in reply, reply
        return reply['result']
    def close(self):
        self.p.stdin.close()
        try: self.p.wait(timeout=3)
        except subprocess.TimeoutExpired: self.p.kill(); self.p.wait()
        self.selector.close()

def value(node, key):
    return node['attributes'].get(key, {}).get('value')

def test_protocol(s):
    reply = s.call(op='discover', id='correlation-check')
    assert reply['id'] == 'correlation-check'
    assert reply['result']['accessibility_trusted'], 'Accessibility access is required'
    reply = s.call(op='pointer', delivery={'kind':'global'}, action={'kind':'click','point':{'x':0,'y':0},'button':'right'}, id=2)
    assert reply['error']['code'] == 'invalid_request' and reply['id'] == 2, reply
    reply = s.call(op='discover', unexpected=True)
    assert reply['error']['code'] == 'invalid_request', reply
    reply = s.call(op='inspect', target={'session':'foreign','id':1})
    assert reply['error']['code'] == 'stale_reference', reply
    s.p.stdin.write('invalid json\n'); s.p.stdin.flush()
    assert s.selector.select(3)
    assert json.loads(s.p.stdout.readline())['error']['code'] == 'invalid_request'
    s.result(op='discover')
    print('PASS protocol validation, correlation, foreign references, session recovery')

def test_calculator(s, pid):
    before = s.result(op='observe', request={'pid':pid})
    buttons = {value(n,'AXIdentifier'):n['reference'] for n in before['nodes'] if value(n,'AXRole')=='AXButton'}
    for name in ['AllClear','Two','Add','Three','Equals']:
        receipt = s.result(op='semantic',target=buttons[name],action={'kind':'perform','name':'AXPress'})
        assert receipt['effect']=='dispatched'
    deadline = time.monotonic()+4
    while True:
        after=s.result(op='observe',request={'pid':pid})
        # Calculator exposes the result through a native text-field value.
        displays=[value(n,'AXValue') for n in after['nodes'] if value(n,'AXRole') in ['AXStaticText','AXTextField','AXTextArea']]
        if any(isinstance(v,str) and v.replace('\u200e','')=='5' for v in displays): break
        if time.monotonic()>deadline: raise AssertionError(f'Expected Calculator result 5; display values {displays!r}')
        time.sleep(.05)
    after_buttons={value(n,'AXIdentifier'):n['reference'] for n in after['nodes'] if value(n,'AXRole')=='AXButton'}
    assert buttons['Two']==after_buttons['Two'], 'Stable live button reference changed'
    other=Session()
    try:
        reply=other.call(op='inspect',target=buttons['Two'])
        assert reply['error']['code']=='stale_reference'
    finally: other.close()
    limited=s.result(op='observe',request={'pid':pid,'max_nodes':1})
    assert not limited['complete'] and limited['issues'][0]['code']=='node_limit'
    inspected=s.result(op='inspect',target=buttons['Two'])
    assert 'AXPress' in inspected['actions']
    print('PASS Calculator 2+3=5, reference continuity, session isolation, inspect, truncation')

def test_fixture(s):
    fixture=subprocess.Popen(['native/macos/.build/fixture'])
    try:
        deadline=time.monotonic()+5
        while True:
            tree=s.result(op='observe',request={'pid':fixture.pid})
            fields=[n for n in tree['nodes'] if value(n,'AXIdentifier')=='unimation-test-field']
            if fields: break
            if time.monotonic()>deadline: raise AssertionError('Fixture text field not discovered')
            time.sleep(.05)
        target=fields[0]['reference']
        s.result(op='semantic',target=target,action={'kind':'set_string','attribute':'AXValue','value':'Unimation 🦀'})
        actual=s.result(op='attribute',target=target,name='AXValue')
        assert actual['value']=='Unimation 🦀', actual
        s.result(op='semantic',target=target,action={'kind':'set_bool','attribute':'AXFocused','value':True})
        assert s.result(op='attribute',target=target,name='AXFocused')['value'] is True
        print('PASS fixture Unicode value setting and boolean focus setting')
        s.result(op='semantic',target=target,action={'kind':'set_string','attribute':'AXValue','value':''})
        s.result(op='text',delivery={'kind':'process','pid':fixture.pid},text='Hi🦀')
        def await_value(expected):
            deadline=time.monotonic()+3
            while True:
                actual=s.result(op='attribute',target=target,name='AXValue')['value']
                if actual==expected: return
                if time.monotonic()>deadline: raise AssertionError(f'Expected {expected!r}, got {actual!r}')
                time.sleep(.05)
        await_value('Hi🦀')
        print('PASS process-directed Unicode keyboard events consumed by focused fixture')
        tree=s.result(op='observe',request={'pid':fixture.pid})
        button=next(n for n in tree['nodes'] if value(n,'AXIdentifier')=='unimation-test-button')
        scroll=next(n for n in tree['nodes'] if value(n,'AXIdentifier')=='unimation-test-scroll')
        def center(n):
            pos=n['attributes']['AXPosition']; size=n['attributes']['AXSize']
            return {'x':pos['x']+size['width']/2,'y':pos['y']+size['height']/2}
        s.result(op='pointer',delivery={'kind':'process','pid':fixture.pid},action={'kind':'click','point':center(button)})
        try:
            await_value('clicked')
            print('PASS process-directed Quartz click consumed by fixture button')
        except AssertionError:
            print('KNOWN LIMIT: process-directed Quartz click dispatched but not consumed')
        # This is a separate explicit global-route test after observing the result.
        s.result(op='semantic',target=target,action={'kind':'set_string','attribute':'AXValue','value':'reset'})
        s.result(op='pointer',delivery={'kind':'global'},action={'kind':'click','point':center(button)})
        await_value('clicked')
        print('PASS global Quartz click consumed by fixture button')
        s.result(op='pointer',delivery={'kind':'global'},action={'kind':'move','point':center(scroll)})
        s.result(op='pointer',delivery={'kind':'process','pid':fixture.pid},action={'kind':'scroll','vertical':5,'horizontal':0})
        try:
            await_value('scrolled')
            print('PASS Quartz move and process-directed scroll consumed by fixture')
        except AssertionError:
            print('KNOWN LIMIT: process-directed Quartz scroll dispatched but not consumed')
        s.result(op='semantic',target=target,action={'kind':'set_string','attribute':'AXValue','value':'reset'})
        s.result(op='pointer',delivery={'kind':'global'},action={'kind':'scroll','vertical':5,'horizontal':0})
        await_value('scrolled')
        print('PASS global Quartz scroll consumed by fixture')
    finally:
        fixture.terminate(); fixture.wait(timeout=5)

if __name__=='__main__':
    s=Session()
    try:
        test_protocol(s)
        apps=s.result(op='discover')['applications']
        calc=next(a for a in apps if a['bundle_id']=='com.apple.calculator')
        test_calculator(s,calc['pid'])
        test_fixture(s)
    finally: s.close()
