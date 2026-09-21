"""Run against a service with CAP_BPF and CAP_PERFMON. Fails if sensors are unavailable."""
import json, urllib.request, subprocess, threading, time, sys
from sse import Stream
base=sys.argv[1] if len(sys.argv)>1 else 'http://127.0.0.1:8080'
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def api(path,body=None,method=None):
    req=urllib.request.Request(base+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'},method=method)
    with opener.open(req,timeout=15) as r:return json.load(r)
p=subprocess.Popen(['tests/targets/bin/activity'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
response=None;thread=None
try:
    pid,peer,address,size=p.stdout.readline().split();pid=int(pid);peer=int(peer);address=int(address,16);size=int(size)
    response=Stream(base+'/api/space/events')
    frames=[]
    def consume():
        try:
            for line in response:
                if line.startswith(b'data: '):
                    value=json.loads(line[6:]);
                    if isinstance(value,dict) and 'ipc' in value:frames.append(value)
        except (OSError,ValueError):pass
    thread=threading.Thread(target=consume,daemon=True);thread.start()
    deadline=time.monotonic()+12
    while time.monotonic()<deadline:
        status=api('/api/space/status')
        if any(str(status.get(sensor,'')).startswith('unavailable') for sensor in ('cpu','ipc')):raise AssertionError(status)
        snapshot=api('/api/space/snapshot')
        if any(n['identity']['pid']==pid for n in snapshot['nodes']):break
        time.sleep(.3)
    time.sleep(1)
    status=api('/api/space/status')
    assert status['cpu']=='observing' and status['ipc']=='observing',status
    p.stdin.write('go\n');p.stdin.flush();time.sleep(6)
    assert frames
    ipc=[e for f in frames for e in f['ipc'] if e['process_id']['pid']==pid and e['write'] and e['bytes']>0]
    from collections import defaultdict
    sends=defaultdict(lambda:[0,0]);receives=defaultdict(lambda:[0,0])
    for f in frames:
        for e in f['ipc']:
            if e['process_id']['pid']==pid and e['write']:sends[e['resource']][0]+=e['bytes'];sends[e['resource']][1]+=e['count']
            if e['process_id']['pid']==peer and not e['write']:receives[e['resource']][0]+=e['bytes'];receives[e['resource']][1]+=e['count']
    pipe_sends=[v for k,v in sends.items() if k.startswith('pipe:') and v==[256,1]]
    socket_sends=[v for k,v in sends.items() if k.startswith('socket:')]
    socket_receives=[v for k,v in receives.items() if k.startswith('socket:')]
    cpu=[e for f in frames for e in f.get('cpu',[]) if e['process_id']['pid']==pid]
    assert cpu and sum(e['runtime_ns'] for e in cpu)>500_000_000,cpu
    assert any(e['running_threads']>0 and e['cpus'] for e in cpu),cpu
    assert any(e['running_threads']==0 for e in cpu),cpu
    assert pipe_sends and len(socket_sends)==3 and all(v==[384,2] for v in socket_sends),sends
    assert len(socket_receives)==3 and all(v==[384,2] for v in socket_receives),receives
    assert any(k.startswith('pipe:') and v==[256,1] for k,v in receives.items()),receives
    print('Live sensors passed: scheduler runtime/current CPU; pipe/UNIX/TCP/UDP send and receive; exact bytes/counts; MSG_PEEK and failed send excluded')

finally:
    if response:response.close(thread)
    if p.poll() is None:p.terminate()
    p.wait(timeout=5)
