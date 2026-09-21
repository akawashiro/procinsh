export const remoteLabel = socket => socket?.remote_hostname ? `${socket.remote_hostname}:${socket.remote.slice(socket.remote.lastIndexOf(":")+1)}` : socket?.remote;
export const key = id => `${id.pid}:${id.start_time_ticks}`;
export function networkGroups(edges) {
  const groups = new Map();
  for (const e of edges) {
    if (e.b || e.shared || !e.socket?.network_peer) continue;
    const id = JSON.stringify([key(e.a.process_id), e.socket.protocol, e.socket.remote]);
    if (!groups.has(id)) groups.set(id, {id, a:e.a, socket:e.socket, members:[], label:''});
    groups.get(id).members.push(e);
  }
  for (const group of groups.values()) {
    group.members.sort((a,b)=>a.a.fd-b.a.fd || a.id.localeCompare(b.id));
    group.label = `${group.socket.protocol} ${remoteLabel(group.socket)} ×${group.members.length}`;
  }
  return groups;
}

export function networkLayout(groups, positions, previous = new Map()) {
  const result = new Map(), occupied = new Set();
  // A global lattice keeps markers apart even for neighboring processes.
  const cell = p => `${p.x}:${p.y}:${p.z}`;
  for (const [id] of groups) if (previous.has(id)) {
    const p = previous.get(id); result.set(id,p); occupied.add(cell(p));
  }
  for (const [id,group] of [...groups].sort(([a],[b])=>a.localeCompare(b))) {
    if (result.has(id)) continue;
    const origin = positions.get(key(group.a.process_id));
    if (!origin) continue;
    let placed=false;
    for(let layer=0;!placed;layer++) for(let slot=0;slot<9;slot++) {
      const p={x:Math.round(origin.x/3)*3+(slot%3-1)*3,
        y:Math.round(origin.y/3)*3+(Math.floor(slot/3)-1)*3,z:12+layer*3};
      if(occupied.has(cell(p))) continue;
      result.set(id,p);occupied.add(cell(p));placed=true;break;
    }
  }
  return result;
}

export function connectionState(e) {
  if(e.shared) return 'Shared FD';
  if(e.candidate) return 'Candidate peer';
  if(e.b) return 'Confirmed process connection';
  if(e.socket?.network_peer) return 'Network destination';
  if(e.socket?.state==='LISTEN') return 'Listening';
  if(e.socket?.protocol.startsWith('UDP')) return 'No destination set';
  return 'Unknown destination';
}
export function layoutMaps(maps) {
  const sorted = [...maps].sort((a,b) => BigInt(a.start)<BigInt(b.start)?-1:1);
  const sizes = sorted.map(m=>Number(BigInt(m.end)-BigInt(m.start)));
  const total=sizes.reduce((a,b)=>a+b,0)||1, gap=.018;
  let z=0;
  return sorted.map((m,i)=>{const h=Math.max(.008,sizes[i]/total*7);const result={...m,z,h};z+=h+gap;return result;}).map((m,_,all)=>({...m,z:m.z/z*8,h:m.h/z*8}));
}
export function addressZ(regions,address) {
  const a=BigInt(address),r=regions.find(r=>BigInt(r.start)<=a&&a<BigInt(r.end));
  if(!r)return null;
  return r.z+Number(a-BigInt(r.start))/Number(BigInt(r.end)-BigInt(r.start))*r.h;
}
export function edgeDirection(edge,event) {
  const a=key(edge.a.process_id)===key(event.process_id)&&edge.a.resource===event.resource;
  const b=edge.b&&key(edge.b.process_id)===key(event.process_id)&&edge.b.resource===event.resource;
  if(edge.shared || (!a&&!b) || (a&&b))return null;
  return a ? (event.write?1:-1) : (event.write?-1:1);
}

export const IPC_PARTICLE_DURATION_MS = 2000;
export const IPC_PARTICLE_STAGGER_MS = 100;
export const REDUCED_MOTION_PARTICLE_DURATION_MS = 150;

export function ipcParticlePlan(operationCount, reducedMotion = false) {
  if (reducedMotion) return {duration:REDUCED_MOTION_PARTICLE_DURATION_MS,offsets:[0]};
  const count = Number.isFinite(operationCount) ? Math.max(1, operationCount) : 1;
  const particles = Math.min(6, Math.max(2, 1 + Math.ceil(Math.log2(count + 1))));
  return {
    duration: IPC_PARTICLE_DURATION_MS,
    offsets: Array.from({length:particles}, (_, index) => index * IPC_PARTICLE_STAGGER_MS),
  };
}

export function cpuGlowLevel(state, now, afterglowMs = 500) {
  if (!state || now < state.last || now - state.last >= afterglowMs) return 0;
  const windowNs = Math.max(1, state.window_ms * 1e6);
  const utilization = Math.min(1, state.runtime_ns / windowNs);
  const activity = Math.min(1, .2 + Math.sqrt(utilization) * .8);
  const peak = state.running_threads > 0
    ? Math.min(1, .82 + Math.log2(state.running_threads + 1) * .12)
    : activity;
  return peak * (1 - (now - state.last) / afterglowMs);
}

export function treeLayout(nodes, xGap = 4.8, yGap = 6.5) {
  const ordered = [...nodes].sort((a, b) =>
    a.identity.pid - b.identity.pid || a.identity.start_time_ticks - b.identity.start_time_ticks);
  const known = new Map(ordered.map(node => [key(node.identity), node]));
  const rank = new Map(ordered.map((node, index) => [key(node.identity), index]));
  const parents = new Map();
  for (const node of ordered) {
    const id = key(node.identity), parent = node.parent_id && key(node.parent_id);
    parents.set(id, parent && parent !== id && known.has(parent) ? parent : null);
  }

  // A corrupt or racing proc snapshot must not make the layout recurse forever.
  for (const node of ordered) {
    let id = key(node.identity);
    const path = [], seen = new Map();
    while (id && parents.has(id)) {
      if (seen.has(id)) {
        const cycle = path.slice(seen.get(id));
        const root = cycle.reduce((a, b) => rank.get(a) < rank.get(b) ? a : b);
        parents.set(root, null);
        break;
      }
      seen.set(id, path.length); path.push(id); id = parents.get(id);
    }
  }

  const children = new Map(ordered.map(node => [key(node.identity), []]));
  for (const [id, parent] of parents) if (parent) children.get(parent).push(id);
  for (const list of children.values()) list.sort((a, b) => rank.get(a) - rank.get(b));
  const roots = ordered.map(node => key(node.identity)).filter(id => !parents.get(id));
  const pack = (layouts, gap) => {
    if (!layouts.length) return {positions:new Map(),width:0,height:0};
    const area = layouts.reduce((sum, layout) => sum + (layout.width + gap) * (layout.height + gap), 0);
    const target = Math.max(40, ...layouts.map(layout => layout.width), Math.sqrt(area) * 1.4);
    const positions = new Map();
    let cursorX = 0, cursorY = 0, rowHeight = 0, width = 0;
    for (const layout of layouts) {
      if (cursorX && cursorX + layout.width > target) {
        cursorX = 0; cursorY += rowHeight + gap; rowHeight = 0;
      }
      for (const [id, position] of layout.positions) positions.set(id,{...position,x:position.x+cursorX,y:position.y+cursorY});
      cursorX += layout.width + gap;
      width = Math.max(width,cursorX-gap);
      rowHeight = Math.max(rowHeight,layout.height);
    }
    return {positions,width,height:cursorY+rowHeight};
  };
  const build = id => {
    const packed = pack(children.get(id).map(build),3);
    const width = Math.max(xGap,packed.width), offsetX=(width-packed.width)/2;
    const positions = new Map([[id,{x:width/2,y:0,depth:0,parent:parents.get(id)}]]);
    for (const [child,position] of packed.positions) positions.set(child,{...position,x:position.x+offsetX,y:position.y+yGap,depth:position.depth+1});
    return {positions,width,height:packed.height?packed.height+yGap:yGap};
  };
  const forest = pack(roots.map(build),9), result=forest.positions;
  const values = [...result.values()];
  const center = values.length ? (Math.min(...values.map(v => v.x)) + Math.max(...values.map(v => v.x))) / 2 : 0;
  for (const value of values) value.x -= center;
  return result;
}

// Keep live identities anchored; only newcomers consume vacant space.
export function stableLayout(nodes, previous = new Map(), xGap = 4.8, yGap = 6.5) {
  const initial = treeLayout(nodes, xGap, yGap), result = new Map();
  for (const [id, place] of initial) {
    const old = previous.get(id);
    if (old) result.set(id, {...place, x:old.x, y:old.y});
  }
  if (!result.size) return initial;
  const buckets = new Map(), cell = (x,y) => `${x}:${y}`;
  const occupy = place => {
    const id = cell(Math.floor(place.x/xGap), Math.floor(place.y/yGap));
    if (!buckets.has(id)) buckets.set(id, []);
    buckets.get(id).push(place);
  };
  const vacant = (x,y) => {
    const cx=Math.floor(x/xGap), cy=Math.floor(y/yGap);
    for(let dx=-1;dx<=1;dx++) for(let dy=-1;dy<=1;dy++) {
      for(const p of buckets.get(cell(cx+dx,cy+dy)) || []) {
        if(Math.abs(p.x-x)<xGap-1e-9 && Math.abs(p.y-y)<yGap-1e-9) return false;
      }
    }
    return true;
  };
  for(const place of result.values()) occupy(place);
  const existing=[...result.values()];
  const outside={x:Math.max(...existing.map(p=>p.x))+xGap,y:Math.min(...existing.map(p=>p.y))};
  // Normalized tree depth also gives a finite, deterministic order for cycles.
  const newcomers=[...initial].filter(([id])=>!result.has(id)).sort((a,b)=>
    a[1].depth-b[1].depth || Number(a[0].split(':')[0])-Number(b[0].split(':')[0]) || a[0].localeCompare(b[0]));
  for(const [id,place] of newcomers) {
    const parent=result.get(place.parent);
    const origin=parent?{x:parent.x,y:parent.y+yGap}:outside;
    let found=null;
    // Expand square rings below the anchor, keeping children below their parent.
    for(let radius=0;!found;radius++) {
      for(let dy=0;dy<=radius&&!found;dy++) for(let dx=-radius;dx<=radius;dx++) {
        if(Math.max(Math.abs(dx),dy)!==radius) continue;
        const x=origin.x+dx*xGap,y=origin.y+dy*yGap;
        if(vacant(x,y)) { found={...place,x,y}; break; }
      }
    }
    result.set(id,found); occupy(found);
  }
  return result;
}

export function userColor(uid) {
  if(uid===null||uid===undefined) return '#889299';
  let hash=Number(uid)>>>0;
  hash=Math.imul(hash^(hash>>>16),0x45d9f3b);
  hash=Math.imul(hash^(hash>>>16),0x45d9f3b);
  const hue=((hash^(hash>>>16))>>>0)%360;
  return `hsl(${hue}, 65%, 65%)`;
}
export const processColors = n => ({real:userColor(n.uid),effective:userColor(n.euid)});

// Recent file activity is independent of the five-second FD topology snapshot.
export const fileKey = event => JSON.stringify([key(event.process_id), event.resource]);
export class RecentFiles {
  constructor(){this.entries=new Map();this.evicted=0;}
  clear(){this.entries.clear();this.evicted=0;}
  prune(now,live){let changed=false;for(const [id,file] of this.entries)if(now-file.last>=30000||!live.has(key(file.process_id))){this.entries.delete(id);changed=true;}return changed;}
  ingest(events,now,live){
    let changed=this.prune(now,live);
    for(const e of events){
      if(!live.has(key(e.process_id))||!(e.bytes>0)||!(e.count>0))continue;
      const id=fileKey(e);let file=this.entries.get(id);
      if(!file){
        const owner=key(e.process_id),owned=[...this.entries.values()].filter(f=>key(f.process_id)===owner);
        const victim=owned.length>=32?owned[0]:this.entries.size>=512?this.entries.values().next().value:null;
        if(victim){this.entries.delete(victim.id);this.evicted++;}
        file={id,process_id:e.process_id,resource:e.resource,path:null,readBytes:0,writeBytes:0,readCount:0,writeCount:0};changed=true;
      }
      if(e.path)file.path=e.path;
      file.label=file.path?.split('/').pop()||file.resource;
      file.last=now;
      file[e.write?'writeBytes':'readBytes']+=e.bytes;
      file[e.write?'writeCount':'readCount']+=e.count;
      this.entries.delete(id);this.entries.set(id,file);
    }
    return changed;
  }
}
export function fileLayout(files,positions,previous=new Map()){
  const groups=new Map([...files].map(([id,f])=>[id,{a:{process_id:f.process_id}}]));
  const prior=new Map([...previous].map(([id,p])=>[id,{...p,z:9-p.z}]));
  return new Map([...networkLayout(groups,positions,prior)].map(([id,p])=>[id,{...p,z:9-p.z}]));
}
