// Build and update Three.js geometry from data and selection state.
import * as T from "three";
import { key, processColors } from "./model.js";
import type {
  Region,
  RenderNode,
  EdgeView,
  NetworkView,
  FileView,
  ConnectionPick,
  SelectionState,
} from "./types.js";
import type {
  SceneInput,
  FileSceneInput,
  RenderView,
  PickingView,
} from "./contracts.js";
function disposeObject(object: T.Object3D) {
  if (
    object instanceof T.Mesh ||
    object instanceof T.Line ||
    object instanceof T.Points
  ) {
    object.geometry.dispose();
    const materials = Array.isArray(object.material)
      ? object.material
      : [object.material];
    for (const material of materials) material.dispose();
  }
}

export function createSpaceScene() {
  const scene = new T.Scene();
  scene.fog = new T.FogExp2(0x03090e, 0.0015);
  let nodes = new Map<string, RenderNode>();
  let edgeViews: EdgeView[] = [],
    networkViews: NetworkView[] = [],
    fileViews = new Map<string, FileView>();
  let parentViews: { parent: string; child: string }[] = [],
    parentLines: T.LineSegments<T.BufferGeometry, T.LineBasicMaterial> | null =
      null;
  let geometryGroup = new T.Group(),
    fileGroup = new T.Group();
  scene.add(geometryGroup, fileGroup);
  let hull: T.InstancedMesh | null = null,
    baseGlow: T.InstancedMesh | null = null,
    haloGlow: T.InstancedMesh | null = null,
    hullIds: string[] = [];
  const dummy = new T.Object3D();
  function syncNodes(input: FileSceneInput) {
    nodes = new Map(
      [...input.nodes].map(([id, n]) => [
        id,
        { ...n, pos: new T.Vector3(n.pos.x, n.pos.y, n.pos.z) },
      ]),
    );
  }
  const selection = new T.LineSegments(
    new T.EdgesGeometry(new T.BoxGeometry(2.45, 2.45, 8.3)),
    new T.LineBasicMaterial({
      color: 0xd1ffce,
      transparent: true,
      opacity: 0.85,
    }),
  );
  selection.visible = false;
  scene.add(selection);
  const edgeSelection = new T.Line(
    new T.BufferGeometry(),
    new T.LineBasicMaterial({
      color: 0xc9fff2,
      transparent: true,
      opacity: 1,
      depthWrite: false,
      blending: T.AdditiveBlending,
    }),
  );
  edgeSelection.visible = false;
  scene.add(edgeSelection);
  function regionColor(r: Region) {
    const p = r.pathname || "";
    return p.includes("[stack")
      ? 0xab8add
      : p === "[heap]"
        ? 0x6cda92
        : r.executable
          ? 0x5ddad4
          : r.writable
            ? 0x347d91
            : 0x244b5e;
  }
  function disposeGroup() {
    geometryGroup.traverse(disposeObject);
    scene.remove(geometryGroup);
    geometryGroup = new T.Group();
    scene.add(geometryGroup);
  }
  function buildScene(input: SceneInput, selectedProcess: string | null) {
    syncNodes(input);
    const { snapshot, network, networkPositions } = input;
    disposeGroup();
    const visible = input.visible;
    hullIds = [...visible];
    hull = new T.InstancedMesh(
      new T.BoxGeometry(2.2, 2.2, 8),
      new T.MeshBasicMaterial({
        color: 0x4bd6bf,
        transparent: true,
        opacity: 0.025,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    hull.count = hullIds.length;
    geometryGroup.add(hull);
    baseGlow = new T.InstancedMesh(
      new T.BoxGeometry(2.28, 2.28, 0.07),
      new T.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.95,
        blending: T.AdditiveBlending,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    baseGlow.count = hullIds.length;
    baseGlow.frustumCulled = false;
    geometryGroup.add(baseGlow);
    haloGlow = new T.InstancedMesh(
      new T.BoxGeometry(3.15, 3.15, 0.025),
      new T.MeshBasicMaterial({
        color: 0xffffff,
        transparent: true,
        opacity: 0.32,
        blending: T.AdditiveBlending,
        depthWrite: false,
      }),
      Math.max(1, hullIds.length),
    );
    haloGlow.count = hullIds.length;
    haloGlow.frustumCulled = false;
    geometryGroup.add(haloGlow);
    parentViews = [];
    const parentPos = [],
      parentColors = [];
    for (const childId of hullIds) {
      const child = nodes.get(childId)!,
        parentId = child.parent_id && key(child.parent_id),
        parent = parentId && nodes.get(parentId);
      if (!parent || !visible.has(parentId)) continue;
      parentViews.push({ parent: parentId, child: childId });
      parentPos.push(
        parent.pos.x,
        parent.pos.y,
        0.06,
        child.pos.x,
        child.pos.y,
        0.06,
      );
      parentColors.push(0.12, 0.28, 0.27, 0.12, 0.28, 0.27);
    }
    const parentGeometry = new T.BufferGeometry();
    parentGeometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(parentPos, 3),
    );
    parentGeometry.setAttribute(
      "color",
      new T.Float32BufferAttribute(parentColors, 3),
    );
    parentLines = new T.LineSegments(
      parentGeometry,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
      }),
    );
    parentLines.renderOrder = -1;
    geometryGroup.add(parentLines);
    const linePos: number[] = [],
      lineColors: number[] = [],
      lineEdges: (ConnectionPick | null)[] = [],
      layerList: { n: RenderNode; r: Region }[] = [];
    let regionLimit = 0;
    const addLine = (
      endpoint: number[],
      peer: number[],
      color: T.ColorRepresentation,
      edge: ConnectionPick | null = null,
    ) => {
      lineEdges.push(edge);
      linePos.push(...endpoint, ...peer);
      const c = new T.Color(color);
      lineColors.push(c.r, c.g, c.b, c.r, c.g, c.b);
    };
    for (let i = 0; i < hullIds.length; i++) {
      const n = nodes.get(hullIds[i])!,
        p = n.pos,
        userColors = processColors(n);
      dummy.position.set(p.x, p.y, 4);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      hull.setMatrixAt(i, dummy.matrix);
      dummy.position.set(p.x, p.y, -0.035);
      dummy.updateMatrix();
      baseGlow.setMatrixAt(i, dummy.matrix);
      baseGlow.setColorAt(i, new T.Color(0));
      dummy.position.z = -0.075;
      dummy.updateMatrix();
      haloGlow.setMatrixAt(i, dummy.matrix);
      haloGlow.setColorAt(i, new T.Color(0));
      for (const z of [0, 8])
        for (let j = 0; j < 4; j++) {
          const corners = [
              [-1.1, -1.1],
              [1.1, -1.1],
              [1.1, 1.1],
              [-1.1, 1.1],
            ],
            a = corners[j],
            b = corners[(j + 1) % 4];
          addLine(
            [p.x + a[0], p.y + a[1], z],
            [p.x + b[0], p.y + b[1], z],
            userColors.real,
          );
        }
      for (const x of [-1.1, 1.1])
        for (const y of [-1.1, 1.1])
          addLine(
            [p.x + x, p.y + y, 0],
            [p.x + x, p.y + y, 8],
            userColors.effective,
          );
      const stride =
        n.regions.length > 64 && hullIds[i] !== selectedProcess
          ? Math.ceil(n.regions.length / 64)
          : 1;
      for (let ri = 0; ri < n.regions.length; ri += stride) {
        if (regionLimit++ >= 120000) break;
        const first = n.regions[ri],
          last = n.regions[Math.min(ri + stride - 1, n.regions.length - 1)];
        layerList.push({ n, r: { ...first, h: last.z + last.h - first.z } });
      }
    }
    const layers = new T.InstancedMesh(
      new T.BoxGeometry(2.12, 2.12, 1),
      new T.MeshBasicMaterial({
        transparent: true,
        opacity: 0.18,
        depthWrite: true,
      }),
      Math.max(1, layerList.length),
    );
    layers.count = layerList.length;
    for (let i = 0; i < layerList.length; i++) {
      const { n, r } = layerList[i];
      dummy.position.set(n.pos.x, n.pos.y, r.z + r.h / 2);
      dummy.scale.set(1, 1, Math.max(0.0001, r.h));
      dummy.updateMatrix();
      layers.setMatrixAt(i, dummy.matrix);
      layers.setColorAt(i, new T.Color(regionColor(r)));
    }
    geometryGroup.add(layers);
    edgeViews = [];
    networkViews = [];
    const grouped = new Set(
      [...network.values()].flatMap((g) => g.members.map((e) => e.id)),
    );
    const visibleGroups = [...network.values()].filter(
      (g) =>
        visible.has(key(g.endpoint.process_id)) && networkPositions.has(g.id),
    );
    const markers = new T.InstancedMesh(
      new T.OctahedronGeometry(0.75),
      new T.MeshBasicMaterial({
        color: 0x68baff,
        transparent: true,
        opacity: 0.2,
      }),
      Math.max(1, visibleGroups.length),
    );
    markers.count = visibleGroups.length;
    markers.userData.network = visibleGroups;
    geometryGroup.add(markers);
    const markerEdges = new T.InstancedMesh(
      new T.OctahedronGeometry(0.75),
      new T.MeshBasicMaterial({
        color: 0x8dd4ff,
        wireframe: true,
        transparent: true,
        opacity: 0.9,
      }),
      Math.max(1, visibleGroups.length),
    );
    markerEdges.count = markers.count;
    markerEdges.instanceMatrix = markers.instanceMatrix;
    geometryGroup.add(markerEdges);
    for (let i = 0; i < visibleGroups.length; i++) {
      const group = visibleGroups[i],
        p = networkPositions.get(group.id)!,
        pos = new T.Vector3(p.x, p.y, p.z),
        owner = nodes.get(key(group.endpoint.process_id))!;
      dummy.position.copy(pos);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      markers.setMatrixAt(i, dummy.matrix);
      const start = owner.pos.clone().setZ(7.5),
        mid = start.clone().lerp(pos, 0.5);
      mid.z += 2;
      const curve = new T.QuadraticBezierCurve3(start, mid, pos);
      networkViews.push({ group, pos, curve });
      for (const e of group.members)
        edgeViews.push({ e, curve, networkId: group.id });
      const pts = curve.getPoints(24),
        pick = {
          ...group,
          peer: null,
          shared: false,
          candidate: false,
          networkId: group.id,
        };
      for (let j = 0; j < 24; j++)
        addLine(pts[j].toArray(), pts[j + 1].toArray(), 0x68baff, pick);
    }
    const dashedPos = [],
      dashedEdges = [];
    for (const e of snapshot.fd_relations) {
      if (grouped.has(e.id)) continue;
      const a = nodes.get(key(e.endpoint.process_id)),
        b = e.peer && nodes.get(key(e.peer.process_id));
      if (
        !a ||
        !visible.has(key(a.identity)) ||
        (b && !visible.has(key(b.identity)))
      )
        continue;
      const start = a.pos
        .clone()
        .add(new T.Vector3(1.12, 0, 1 + (e.endpoint.fd % 12) * 0.11));
      const end = b
        ? b.pos
            .clone()
            .add(new T.Vector3(-1.12, 0, 1 + (e.peer!.fd % 12) * 0.11))
        : start.clone().add(new T.Vector3(3.5, -2.5, -0.6));
      const mid = start.clone().lerp(end, 0.5);
      mid.z = 0.22;
      const curve = new T.QuadraticBezierCurve3(start, mid, end);
      edgeViews.push({ e, curve });
      const pts = curve.getPoints(16);
      for (let j = 0; j < 16; j++) {
        if (e.candidate || e.shared) {
          if (j % 2 === 0) {
            dashedPos.push(...pts[j], ...pts[j + 1]);
            dashedEdges.push(e);
          }
        } else
          addLine(
            pts[j].toArray(),
            pts[j + 1].toArray(),
            e.peer ? 0x236857 : 0x24404c,
            e,
          );
      }
    }
    const lines = new T.BufferGeometry();
    lines.setAttribute("position", new T.Float32BufferAttribute(linePos, 3));
    lines.setAttribute("color", new T.Float32BufferAttribute(lineColors, 3));
    const solidLines = new T.LineSegments(
      lines,
      new T.LineBasicMaterial({
        vertexColors: true,
        transparent: true,
        opacity: 0.75,
        depthWrite: false,
        blending: T.AdditiveBlending,
      }),
    );
    solidLines.userData.fd_relations = lineEdges;
    geometryGroup.add(solidLines);
    const dashed = new T.BufferGeometry();
    dashed.setAttribute("position", new T.Float32BufferAttribute(dashedPos, 3));
    const dashedLines = new T.LineSegments(
      dashed,
      new T.LineBasicMaterial({
        color: 0x54828b,
        transparent: true,
        opacity: 0.3,
        depthWrite: false,
      }),
    );
    dashedLines.userData.fd_relations = dashedEdges;
    geometryGroup.add(dashedLines);
    refreshFileScene(input);
  }
  function refreshFileScene(input: FileSceneInput) {
    syncNodes(input);
    const { files: recentFiles, filePositions } = input;
    fileGroup.traverse(disposeObject);
    scene.remove(fileGroup);
    fileGroup = new T.Group();
    scene.add(fileGroup);
    const visible = input.visible,
      files = [...recentFiles.values()].filter((f) =>
        visible.has(key(f.process_id)),
      );
    const markers = new T.InstancedMesh(
      new T.BoxGeometry(1.5, 0.22, 1.8),
      new T.MeshBasicMaterial({
        color: 0xe2d5a0,
        transparent: true,
        opacity: 0.65,
      }),
      Math.max(1, files.length),
    );
    markers.count = files.length;
    markers.userData.files = files;
    fileGroup.add(markers);
    fileViews = new Map();
    const vertices = [],
      paths = [];
    for (let i = 0; i < files.length; i++) {
      const file = files[i],
        p = filePositions.get(file.id)!,
        owner = nodes.get(key(file.process_id))!,
        pos = new T.Vector3(p.x, p.y, p.z);
      dummy.position.copy(pos);
      dummy.scale.set(1, 1, 1);
      dummy.updateMatrix();
      markers.setMatrixAt(i, dummy.matrix);
      const start = owner.pos.clone().setZ(0.5),
        mid = start.clone().lerp(pos, 0.5);
      mid.y -= 1.5;
      const curve = new T.QuadraticBezierCurve3(start, mid, pos);
      fileViews.set(file.id, { file, pos, curve });
      const points = curve.getPoints(16);
      for (let j = 0; j < 16; j++) {
        vertices.push(...points[j], ...points[j + 1]);
        paths.push(file.id);
      }
    }
    const geometry = new T.BufferGeometry();
    geometry.setAttribute(
      "position",
      new T.Float32BufferAttribute(vertices, 3),
    );
    const lines = new T.LineSegments(
      geometry,
      new T.LineBasicMaterial({
        color: 0xc9bb88,
        transparent: true,
        opacity: 0.5,
        depthWrite: false,
      }),
    );
    lines.userData.filePaths = paths;
    fileGroup.add(lines);
  }
  function updateParentSelection(selectionState: SelectionState) {
    if (!parentLines?.geometry.attributes.color) return;
    const active = new Set();
    if (selectionState.process) {
      let child = selectionState.process;
      const seen = new Set();
      while (child && !seen.has(child)) {
        seen.add(child);
        const parentId = nodes.get(child)?.parent_id;
        const parent: string | null | undefined = parentId && key(parentId);
        if (!parent || !nodes.has(parent)) break;
        active.add(`${parent}>${child}`);
        child = parent;
      }
      for (const [id, n] of nodes) {
        const parent = n.parent_id && key(n.parent_id);
        if (parent === selectionState.process)
          active.add(`${selectionState.process}>${id}`);
      }
    }
    const colors = parentLines.geometry.attributes.color;
    for (let i = 0; i < parentViews.length; i++) {
      const view = parentViews[i],
        bright = active.has(`${view.parent}>${view.child}`);
      const color: [number, number, number] = bright
        ? [0.55, 1, 0.82]
        : [0.12, 0.28, 0.27];
      colors.setXYZ(i * 2, ...color);
      colors.setXYZ(i * 2 + 1, ...color);
    }
    colors.needsUpdate = true;
  }
  function updateSelection(selectionState: SelectionState) {
    const n = nodes.get(selectionState.process ?? "");
    selection.visible = !!n;
    if (n) selection.position.set(n.pos.x, n.pos.y, 4);
    const view = selectionState.file
      ? fileViews.get(selectionState.file)
      : selectionState.network
        ? networkViews.find((v) => v.group.id === selectionState.network)
        : edgeViews.find((v) => v.e.id === selectionState.connection);
    edgeSelection.visible = !!view;
    if (view) edgeSelection.geometry.setFromPoints(view.curve.getPoints(32));
    updateParentSelection(selectionState);
  }
  function cpuGlowVisual(id: string) {
    const index = hullIds.indexOf(id);
    if (index < 0 || !baseGlow) return null;
    const color = new T.Color();
    baseGlow.getColorAt(index, color);
    return { r: color.r, g: color.g, b: color.b };
  }
  function parentLineVisual(parent: string, child: string) {
    const index = parentViews.findIndex(
      (view) => view.parent === parent && view.child === child,
    );
    if (index < 0 || !parentLines) return null;
    const color = parentLines.geometry.attributes.color;
    return {
      r: color.getX(index * 2),
      g: color.getY(index * 2),
      b: color.getZ(index * 2),
    };
  }

  return {
    root: scene,
    rebuild: buildScene,
    refreshFiles: refreshFileScene,
    updateSelection,
    cpuGlowVisual,
    parentLineVisual,
    setFogDensity(density: number) {
      (scene.fog as T.FogExp2).density = density;
    },
    renderView(): RenderView {
      return {
        nodes,
        hullIds,
        networkViews,
        fileViews,
        edgeViews,
        baseGlow,
        haloGlow,
      };
    },
    pickingView(): PickingView {
      return {
        nodes,
        hullIds,
        hull,
        objects: [...geometryGroup.children, ...fileGroup.children],
      };
    },
    dispose() {
      scene.traverse(disposeObject);
    },
  };
}
export type SpaceScene = ReturnType<typeof createSpaceScene>;
