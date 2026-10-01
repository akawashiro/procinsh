"""Run against a service with CAP_BPF and CAP_PERFMON. Fails if sensors are unavailable."""
import json, subprocess, threading, time, sys
from sse import Stream
base=sys.argv[1] if len(sys.argv)>1 else 'http://127.0.0.1:8080'

p=subprocess.Popen(['tests/targets/bin/activity'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
response=None;thread=None
try:
    pid,peer,address,size=p.stdout.readline().split();pid=int(pid);peer=int(peer);address=int(address,16);size=int(size)
    response=Stream(base+'/api/system/events')
    frames=[]; latest={"snapshot": {}, "status": {}}
    def consume():
        try:
            for line in response:
                if line.startswith(b'data: '):
                    value=json.loads(line[6:]);
                    if isinstance(value,dict):
                        if 'processes' in value:latest['snapshot']=value
                        if 'ipc' in value:
                            frames.append(value)
                            latest['status']=value['status']
        except (OSError,ValueError):pass
    thread=threading.Thread(target=consume,daemon=True);thread.start()
    deadline=time.monotonic()+12
    while time.monotonic()<deadline:
        status=latest['status']
        if any(status.get(sensor,{}).get('state')=='unavailable' for sensor in ('cpu','ipc')):raise AssertionError(status)
        snapshot=latest['snapshot']
        if all(status.get(sensor,{}).get('state')=='observing' for sensor in ('cpu','ipc')) and any(n['identity']['pid']==pid for n in snapshot.get('processes',[])):break
        time.sleep(.3)
    else:raise AssertionError(('sensor/snapshot did not become ready',latest))
    status=latest['status']
    assert status['cpu']['state']=='observing' and status['ipc']['state']=='observing',status
    p.stdin.write('go\n');p.stdin.flush();time.sleep(6)
    assert frames
    ipc=[e for f in frames for e in f['ipc'] if e['process_id']['pid']==pid and e['write'] and e['bytes']>0]
    from collections import defaultdict
    sends=defaultdict(lambda:[0,0]);receives=defaultdict(lambda:[0,0])
    def resource_key(resource):
        return (resource['kind'], resource['device']['major'], resource['device']['minor'], resource['inode'])
    for f in frames:
        for e in f['ipc']:
            if e['process_id']['pid']==pid and e['write']:sends[resource_key(e['resource'])][0]+=e['bytes'];sends[resource_key(e['resource'])][1]+=e['count']
            if e['process_id']['pid']==peer and not e['write']:receives[resource_key(e['resource'])][0]+=e['bytes'];receives[resource_key(e['resource'])][1]+=e['count']
    pipe_sends=[v for k,v in sends.items() if k[0]=='pipe' and v==[256,1]]
    socket_sends=[v for k,v in sends.items() if k[0]=='socket']
    socket_receives=[v for k,v in receives.items() if k[0]=='socket']
    cpu=[e for f in frames for e in f.get('cpu',[]) if e['process_id']['pid']==pid]
    assert cpu and sum(e['runtime_ns'] for e in cpu)>500_000_000,cpu
    assert any(e['running_threads']>0 and e['cpus'] for e in cpu),cpu
    assert any(e['running_threads']==0 for e in cpu),cpu
    assert pipe_sends and len(socket_sends)==3 and all(v==[384,2] for v in socket_sends),sends
    assert len(socket_receives)==3 and all(v==[384,2] for v in socket_receives),receives
    assert any(k[0]=='pipe' and v==[256,1] for k,v in receives.items()),receives
    print('Live sensors passed: scheduler runtime/current CPU; pipe/UNIX/TCP/UDP send and receive; exact bytes/counts; MSG_PEEK and failed send excluded')

finally:
    if response:response.close(thread)
    if p.poll() is None:p.terminate()
    p.wait(timeout=5)
