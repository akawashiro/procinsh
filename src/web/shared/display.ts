// Shared formatting. Values remain structured in models and API contracts.
export namespace Display {
  export function permissions(m: import('./api-types.js').MappingPermissions): string {
    return `${m.readable?'r':'-'}${m.writable?'w':'-'}${m.executable?'x':'-'}${m.private?'p':'s'}`;
  }
  export function mapping(m: import('./api-types.js').RegisterMapping): string {
    return `${m.pathname ?? '[anonymous]'} [${permissions(m)}]`;
  }
  export function scheduler(p: import('./api-types.js').SchedulerPolicy): string {
    return p.kind === 'unknown' ? `UNKNOWN (${p.code})` : p.kind.toUpperCase();
  }
  export function affinity(ranges: import('./api-types.js').CpuRange[] | null): string {
    return ranges === null ? 'N/A' : ranges.map(r=>r.start===r.end ? String(r.start) : `${r.start}-${r.end}`).join(',');
  }
  export function address(a: import('./api-types.js').InetAddress | null | undefined): string {
    return a ? `${a.ip.includes(':') ? `[${a.ip}]` : a.ip}:${a.port}` : '';
  }
  export function protocol(p: import('./api-types.js').SocketProtocol | null | undefined): string {
    if (!p) return '';
    if (p.kind === 'unix') {
      const type = p.socket_type.kind;
      return type === 'unknown' ? 'UNIX' : `UNIX ${type.toUpperCase()}`;
    }
    return `${p.kind.toUpperCase()}${p.family === 'ipv6' ? '6' : ''}`;
  }
  export function state(s: import('./api-types.js').SocketState | null | undefined): string {
    if (!s) return '';
    return s.kind === 'unknown_inet' || s.kind === 'unknown_unix' ? s.code.toString(16).toUpperCase().padStart(2,'0') : s.kind.toUpperCase();
  }
  export function access(a: import('./api-types.js').FdAccess): string {
    return a === 'unknown' ? 'N/A' : a === 'read_write' ? 'read/write' : a;
  }
}

export const num = (v: number | null | undefined, digits = 1) =>
  v == null
    ? "N/A"
    : v.toLocaleString("en-US", { maximumFractionDigits: digits });
export const percent = (v: number | null | undefined) =>
  v == null ? "N/A" : `${num(v)}%`;
export function bytes(v: number | null | undefined) {
  if (v == null) return "N/A";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${num(v)} ${units[i]}`;
}
export const rate = (v: number | null | undefined) =>
  v == null ? "N/A" : `${num(v)}/s`;
export const byteRate = (v: number | null | undefined) =>
  v == null ? "N/A" : `${bytes(v)}/s`;
