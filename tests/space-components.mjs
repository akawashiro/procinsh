// Exercise SPACE components with plain inputs, Three.js objects, and callbacks.
import assert from 'node:assert/strict';
import {readFile, readdir} from 'node:fs/promises';
import {register} from 'node:module';
import ts from 'typescript';

// Browser absolute vendor URLs resolve to the same bundled Three.js in Node.
register('data:text/javascript,' + encodeURIComponent(`
  export function resolve(specifier, context, nextResolve) {
    if (specifier.startsWith('/vendor/'))
      return {url: new URL(specifier.slice('/vendor/'.length), ${JSON.stringify(new URL('../src/web/vendor/', import.meta.url).href)} ).href, shortCircuit: true};
    return nextResolve(specifier, context);
  }
`), import.meta.url);
const T = await import('../src/web/vendor/three.module.js');
const {createSpaceScene} = await import('../dist/web/space/scene.js');
const {SpaceSelection, bindSelection} = await import('../dist/web/space/selection.js');
const {createSpaceCamera} = await import('../dist/web/space/camera.js');
const {createSpaceDetails} = await import('../dist/web/space/details.js');
const {createSpaceRenderer} = await import('../dist/web/space/renderer.js');

// Check erased type imports as well as runtime imports to keep the boundary intact.
for (const name of await readdir(new URL('../src/web/space/', import.meta.url))) {
  if (!name.endsWith('.ts') || name === 'app.ts') continue;
  const source = ts.createSourceFile(name, await readFile(new URL(`../src/web/space/${name}`, import.meta.url), 'utf8'), ts.ScriptTarget.Latest);
  for (const statement of source.statements) {
    if (!ts.isImportDeclaration(statement) && !ts.isExportDeclaration(statement)) continue;
    const path = statement.moduleSpecifier?.text;
    if (path?.startsWith('./'))
      assert.ok(['./types.js', './contracts.js', './model.js', './dom-types.js'].includes(path), `${name} depends on ${path}`);
  }
}

const emptySelection = () => ({process: null, connection: null, network: null, file: null});
const identity = {pid: 101, start_time_ticks: 1};
const node = {identity, parent_id: null, name: 'writer', pos: {x: 0, y: 0, z: 0}, regions: []};
const file = {id: 'file', process_id: identity, label: 'data.txt', path: '/tmp/data.txt', file: {device: {major: 8, minor: 1}, inode: '9', generation: 0}, readBytes: 8, writeBytes: 12, readCount: 1, writeCount: 2};
const nodes = new Map([['101:1', node]]);

{
  const scene = createSpaceScene();
  const input = {nodes, snapshot: {processes: [node], fd_relations: []}, network: new Map(), networkPositions: new Map(), files: new Map([['file', file]]), filePositions: new Map([['file', {x: 3, y: 0, z: -3}]]), visible: new Set(nodes.keys())};
  scene.rebuild(input, null);
  assert.deepEqual(scene.renderView().hullIds, ['101:1']);
  assert.equal(scene.renderView().fileViews.get('file').pos.z, -3);
  assert.deepEqual(node.pos, {x: 0, y: 0, z: 0}, 'scene does not convert the input coordinates in place');
  const picked = scene.pickingView();
  let disposed = false;
  picked.hull.geometry.addEventListener('dispose', () => disposed = true);
  scene.rebuild({...input, visible: new Set()}, null);
  assert.equal(disposed, true, 'rebuild disposes old picking geometry');
  assert.notEqual(scene.pickingView().hull, picked.hull, 'consumers must read current picking objects');
  assert.equal(scene.renderView().fileViews.size, 0, 'search filters file geometry with its owner');
  scene.rebuild(input, null);
  scene.updateSelection({...emptySelection(), file: 'file'});
  scene.refreshFiles({nodes, visible: input.visible, files: new Map(), filePositions: new Map()});
  scene.updateSelection(emptySelection());
  assert.equal(scene.renderView().fileViews.size, 0, 'file refresh accepts only the needed values');
  scene.dispose();
}
{
  const selection = new SpaceSelection();
  for (const [kind, target] of [['process', 'processes'], ['connection', 'connections'], ['network', 'networks'], ['file', 'files']]) {
    const targets = {processes: new Set(), connections: new Set(), networks: new Set(), files: new Set()};
    selection.choose(kind, 'id');
    targets[target].add('id');
    selection.retain(targets);
    assert.equal(selection[kind], 'id');
    targets[target].clear();
    selection.retain(targets);
    assert.equal(selection.active, false, `${kind} selection clears when its identity disappears`);
  }
}
Object.assign(globalThis, {innerWidth: 800, innerHeight: 600, devicePixelRatio: 1});
const pointer = (canvas, type, x = 400, y = 300) => {
  const event = new Event(type);
  Object.assign(event, {clientX: x, clientY: y});
  canvas.dispatchEvent(event);
};
{
  // Picking has no scene, store, or camera controller; only current Three.js objects.
  const canvas = new EventTarget(), hover = {style: {}};
  const camera = new T.PerspectiveCamera(45, 800 / 600, .1, 100);
  camera.position.set(0, 0, 10); camera.lookAt(0, 0, 0); camera.updateMatrixWorld();
  const hull = new T.InstancedMesh(new T.BoxGeometry(2, 2, 2), new T.MeshBasicMaterial(), 1);
  hull.setMatrixAt(0, new T.Matrix4()); hull.updateMatrixWorld();
  let input = {view: {hull, hullIds: ['101:1'], nodes: new Map([['101:1', {...node, pos: new T.Vector3()}]]), objects: []}, files: new Map(), edgeStats: new Map()};
  const events = [];
  const unbind = bindSelection(canvas, hover, {camera, read: () => input, actions: {
    process: (...args) => events.push(['process', ...args]),
    connection: id => events.push(['connection', id]), network: id => events.push(['network', id]),
    file: id => events.push(['file', id]), clear: () => events.push(['clear']),
  }, hover: state => events.push(['hover', state])});
  pointer(canvas, 'pointerdown'); pointer(canvas, 'pointerup'); pointer(canvas, 'dblclick');
  assert.deepEqual(events.splice(0), [['process', '101:1'], ['process', '101:1', true]]);
  pointer(canvas, 'pointermove');
  assert.match(hover.textContent, /writer \/ 101/);
  assert.equal(hover.hidden, false);
  const marker = new T.InstancedMesh(new T.BoxGeometry(2, 2, 2), new T.MeshBasicMaterial(), 1);
  marker.setMatrixAt(0, new T.Matrix4()); marker.updateMatrixWorld(); marker.userData.files = [file];
  input = {...input, view: {...input.view, hull: null, objects: [marker]}, files: new Map([['file', file]])};
  pointer(canvas, 'pointerdown'); pointer(canvas, 'pointerup'); pointer(canvas, 'pointermove');
  assert.ok(events.some(e => e[0] === 'file' && e[1] === 'file'), 'new geometry is read on the next click');
  assert.deepEqual(events.at(-1), ['hover', {hoveredNetwork: null, hoveredFile: 'file'}]);
  assert.match(hover.textContent, /READ 8 bytes · WRITE 12 bytes/);
  input = {...input, view: {...input.view, objects: []}};
  pointer(canvas, 'pointermove');
  assert.equal(hover.hidden, true);
  assert.deepEqual(events.at(-1), ['hover', {hoveredNetwork: null, hoveredFile: null}]);
  unbind(); const count = events.length;
  pointer(canvas, 'pointerdown'); pointer(canvas, 'pointerup'); pointer(canvas, 'pointermove');
  assert.equal(events.length, count, 'disposing removes pointer handlers');
  for (const mesh of [hull, marker]) {mesh.geometry.dispose(); mesh.material.dispose();}
}
{
  const canvas = new EventTarget();
  canvas.style = {}; canvas.getRootNode = () => new EventTarget();
  const camera = createSpaceCamera(canvas);
  const positions = {processes: [{x: 0, y: 0, z: 0}], networks: [{x: 1000, y: 0, z: 12}], files: [{x: -100, y: 0, z: -3}]};
  const density = camera.adaptWorld(positions);
  assert.equal(camera.camera.far, 4400);
  assert.equal(camera.controls.maxDistance, 2200);
  assert.equal(density, .8 / 1100, 'camera returns fog density for the app to wire');
  camera.fit(positions);
  assert.deepEqual(camera.view().target, [450, 0, 4.5]);
  camera.focus({x: 7, y: 8, z: 0});
  assert.deepEqual(camera.view().target, [7, 8, 4]);
  const before = camera.view();
  camera.fit({processes: [], networks: [], files: []});
  assert.deepEqual(camera.view(), before, 'empty bounds leave the camera in place');
  camera.dispose();
}

// A minimal DOM and WebGL boundary lets UI/animation run without other components.
class Element {
  children = []; style = {}; dataset = {}; hidden = false; textContent = '';
  append(...children) {for (const child of children) {child.parent = this; this.children.push(child);}}
  replaceChildren(...children) {this.children = []; this.append(...children);}
  isEqualNode(other) {return this.tag === other.tag && this.textContent === other.textContent && this.href === other.href && this.children.length === other.children.length && this.children.every((c, i) => c.isEqualNode(other.children[i]));}
  replaceWith(other) {this.parent.children[this.parent.children.indexOf(this)] = other; other.parent = this.parent;}
  remove() {this.parent.children.splice(this.parent.children.indexOf(this), 1);}
  get lastElementChild() {return this.children.at(-1);}
}
const context = {setTransform() {}, clearRect() {}, strokeText() {}, fillText() {}, beginPath() {}, arc() {}, fill() {}, measureText() {return {width: 20};}};
globalThis.document = {hidden: false, createElement(tag) {const element = new Element(); element.tag = tag; element.getContext = () => context; return element;}, createTextNode(text) {const element = new Element(); element.textContent = text; return element;}, createDocumentFragment: () => new Element()};
{
  const elements = new Map();
  const get = id => {if (!elements.has(id)) elements.set(id, new Element()); return elements.get(id);};
  const actions = [];
  const details = createSpaceDetails({connection: id => actions.push(['connection', id]), network: id => actions.push(['network', id])}, get);
  const data = {nodes, snapshot: {fd_relations: []}, network: new Map(), edgeStats: new Map(), files: new Map([['file', file]])};
  details.update(data, {...emptySelection(), process: '101:1'});
  assert.equal(get('name').textContent, 'writer');
  assert.equal(get('inspect').href, '/process/101?start_time_ticks=1');
  details.update(data, {...emptySelection(), file: 'file'});
  assert.match(get('connection-facts').textContent, /READ: 8 bytes.*WRITE: 12 bytes/);
  const link = get('connection-endpoints').children[2];
  details.update(data, {...emptySelection(), file: 'file'});
  assert.equal(get('connection-endpoints').children[2], link, 'live details retain the attached process link');
  const edge = {id: 'connection', endpoint: {process_id: identity, fd: 3, fd_count: 1}, socket: {protocol: {kind: 'tcp'}, state: {kind: 'established'}}, label: 'destination'};
  const group = {id: 'network', endpoint: edge.endpoint, socket: edge.socket, members: [edge], label: 'destination'};
  const withNetwork = {...data, network: new Map([['network', group]])};
  details.update(withNetwork, {...emptySelection(), network: 'network'});
  const row = get('connection-endpoints').children[0].children[1];
  row.children[0].onclick();
  assert.deepEqual(actions, [['connection', 'connection']], 'detail buttons emit actions');
  details.update(data, emptySelection());
  assert.equal(get('details').hidden, true);
}
{
  let pendingFrame, frameCount = 0, cancelled = 0, renders = 0;
  globalThis.requestAnimationFrame = fn => {pendingFrame = fn; return ++frameCount;};
  globalThis.cancelAnimationFrame = () => {pendingFrame = undefined; cancelled++;};
  globalThis.matchMedia = () => ({matches: false});
  class WebGLRenderer {
    setPixelRatio() {} setSize() {} setClearColor() {} dispose() {}
    render() {renders++;}
  }
  let view = {nodes: new Map(), hullIds: [], networkViews: [], fileViews: new Map(), edgeViews: [], baseGlow: null, haloGlow: null};
  const curve = new T.QuadraticBezierCurve3(new T.Vector3(), new T.Vector3(0, 1, 0), new T.Vector3(1, 1, 0));
  const root = new T.Scene(), camera = new T.PerspectiveCamera();
  const beforeFrames = [];
  const renderer = createSpaceRenderer({graphics: {...T, WebGLRenderer}, canvas: {}, labelCanvas: {getContext: () => context}, fpsLabel: {}, failure: {}, scene: root, camera, view: () => view, selection: () => ({...emptySelection(), hoveredNetwork: null, hoveredFile: null}), cpuGlows: new Map(), beforeFrame: now => {beforeFrames.push(now);}});
  view = {...view, fileViews: new Map([['file', {file, pos: new T.Vector3(), curve}]]), edgeViews: [{e: {id: 'connection'}, networkId: 'network', curve}]};
  renderer.activity({now: 1000, routes: [{kind: 'file', id: 'file', direction: 1, count: 1}, {kind: 'connection', id: 'connection', direction: -1, count: 1}]});
  assert.equal(renderer.fileParticles().length, 2);
  assert.equal(renderer.networkParticles().length, 2);
  renderer.retainParticles(new Set());
  assert.equal(renderer.networkParticles().length, 0, 'retention uses supplied network identities');
  view = {...view, fileViews: new Map()}; renderer.retainParticles(new Set());
  assert.equal(renderer.fileParticles().length, 0, 'retention reads replaced geometry');
  renderer.resume(); renderer.resume();
  assert.equal(frameCount, 1, 'resume creates only one loop');
  pendingFrame(2000);
  assert.deepEqual(beforeFrames, [2000]); assert.equal(renders, 1);
  document.hidden = true; pendingFrame(3000);
  assert.deepEqual(beforeFrames, [2000], 'hidden rendering skips app updates');
  document.hidden = false; renderer.pause();
  assert.equal(cancelled, 1); assert.equal(pendingFrame, undefined);
  renderer.resume(); pendingFrame(4000);
  assert.deepEqual(beforeFrames, [2000, 4000]);
  renderer.dispose(); assert.equal(root.children.length, 0);
}
console.log('SPACE components passed: import boundaries, plain scene inputs, current picking geometry, selection retention, camera bounds, detail links/actions, and renderer callbacks/lifecycle.');
