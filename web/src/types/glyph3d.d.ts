declare module "@glyph3d/core" {
  import * as THREE from "three";

  export interface GlyphEngineOptions {
    fontUrl?: string;
    fonts?: Array<{
      url?: string;
      buffer?: ArrayBuffer | Uint8Array;
      name?: string;
    }>;
    wasmUrl?: string;
    wasmBinary?: ArrayBuffer | Uint8Array;
    fontFamily?: string;
    fontSize?: number;
    atlasSize?: number;
    primeRanges?: Array<[number, number]>;
    encodeRanges?: Array<[number, number]>;
    onStage?: (stage: string) => void;
    cache?: boolean;
    coreAssetBase?: string;
  }

  export class GlyphAtlas {
    constructor(fontFamily?: string, fontSize?: number, atlasSize?: number);
    fontFamily: string;
    fontSize: number;
    atlasSize: number;
    dispose(): void;
  }

  export interface CodeGridOptions {
    name?: string;
    maxChars?: number;
    showBackground?: boolean;
    backgroundColor?: number;
    backgroundOpacity?: number;
    backgroundPadding?: number;
    showFilename?: boolean;
    filenameColor?: { r: number; g: number; b: number };
    textColor?: { r: number; g: number; b: number };
    gridScale?: number;
    worldScale?: number;
    occluder?: boolean;
    layout?: Record<string, unknown>;
    padding?: number;
    windowRows?: number;
    windowMarginRows?: number;
  }

  export class CodeGrid extends THREE.Group {
    constructor(
      scene: THREE.Scene | null,
      atlas: GlyphAtlas,
      options?: CodeGridOptions,
    );
    scene: THREE.Scene | null;
    atlas: GlyphAtlas;
    lines: string[];
    content: string;
    filename?: string;
    _renderer?: InstancedField;
    _memoryEdges?: MemoryEdges;
    loadText(text: string, filename?: string): Promise<this>;
    loadFile(filename: string, text: string): Promise<this>;
    highlightRange(
      startLine: number,
      startCol: number,
      endLine: number,
      endCol: number,
      color: { r: number; g: number; b: number },
      fillOpacity?: number,
    ): void;
    clearLineHighlight(line: number): void;
    clearAllHighlights(): void;
    clear(): this;
    dispose(): void;
    getLocalBounds(): THREE.Box3;
    getBounds(): THREE.Box3;
  }

  export interface HexViewOptions {
    cols?: number;
    baseOffset?: number;
    addrWidth?: number;
    ascii?: boolean;
  }

  export function bytesToHexView(
    bytes: Uint8Array,
    options?: HexViewOptions,
  ): string;

  export function byteColor(byte: number): {
    r: number;
    g: number;
    b: number;
  };

  export interface PointerOptions {
    windowOffset?: number;
    width?: number;
    align?: number;
    minValue?: number;
  }

  export function findPointers(
    bytes: Uint8Array,
    options?: PointerOptions,
  ): Array<{ from: number; to: number }>;

  export interface DecorateMemoryGridOptions {
    cols?: number;
    windowOffset?: number;
  }

  export function decorateMemoryGrid(
    grid: CodeGrid,
    bytes: Uint8Array,
    options?: DecorateMemoryGridOptions,
  ): { colored: number; edges: number };

  export class MemoryEdges {
    constructor(
      grid: CodeGrid,
      options?: {
        max?: number;
        color?: { r: number; g: number; b: number };
      },
    );
    setEdges(
      edges: Array<{
        from: { x: number; y: number; z: number };
        to: { x: number; y: number; z: number };
      }>,
    ): void;
    clear(): void;
    dispose(): void;
  }

  export class InstancedSlotBuffer {
    constructor(initialCapacity?: number);
    _capacity: number;
    _count: number;
    buffer: ArrayBuffer;
    ensureCapacity(minSlots: number): void;
    createInterleavedBuffer(
      usage?: THREE.Usage,
    ): THREE.InstancedInterleavedBuffer;
    static bindGeometryAttributes(
      geometry: THREE.BufferGeometry,
      interleavedBuffer: THREE.InterleavedBuffer,
    ): void;
  }

  export class InstancedField {
    constructor(
      scene: THREE.Scene | null,
      atlas?: GlyphAtlas,
      options?: Record<string, unknown>,
    );
    geometry: THREE.InstancedBufferGeometry;
    instanceMesh: THREE.Mesh;
    _interleavedBuffer: THREE.InstancedInterleavedBuffer;
    setSlots(slotBuffer: InstancedSlotBuffer, count: number): void;
  }

  export function bootGlyphEngine(
    options?: GlyphEngineOptions,
  ): Promise<GlyphAtlas>;
}
