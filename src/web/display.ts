// Shared formatting. Values remain structured in models and API contracts.
namespace Display {
  export function scheduler(p: import('./api-types.js').SchedulerPolicy): string {
    return p.kind === 'unknown' ? `UNKNOWN (${p.code})` : p.kind.toUpperCase();
  }
  export function affinity(ranges: import('./api-types.js').CpuRange[] | null): string {
    return ranges === null ? 'N/A' : ranges.map(r=>r.start===r.end ? String(r.start) : `${r.start}-${r.end}`).join(',');
  }
  export function signal(signal: import('./api-types.js').Signal): string {
    const name=signal.name === 'RT' ? `RT (kernel ${signal.number})` : signal.name;
    return `${name} [${signal.number}]`;
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
// app.ts is intentionally a classic script; the space model imports this file.
(globalThis as unknown as {Display: typeof Display}).Display = Display;
