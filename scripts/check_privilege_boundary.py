#!/usr/bin/env python3
"""Review guard for the application module closure, not an FFI security proof."""
from pathlib import Path
import re
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ALLOWED = {
    'src/main.rs', 'src/lib.rs', 'src/ipc.rs', 'src/ipc/schema.rs', 'src/ipc/transport.rs',
    'src/http_server.rs', 'src/http_server/api.rs',
    'src/http_server/api/process.rs', 'src/http_server/api/system.rs',
    'src/http_server/api/router.rs', 'src/http_server/api/stream.rs',
    'src/http_server/client.rs', 'src/http_server/delivery.rs',
    'src/http_server/launch.rs', 'src/http_server/privileges.rs',
    'src/http_server/server.rs', 'src/http_server/state.rs',
    'src/http_server/middleware.rs', 'src/http_server/router.rs',
    'src/http_server/web.rs',
}
# These two files may manipulate only their own credentials and exec endpoint.
LIBC = {
    'src/http_server/launch.rs': {'dup2', 'fcntl', 'F_SETFD', 'syscall', 'SYS_close_range'},
    'src/http_server/privileges.rs': {
        'c_long', 'c_ulong', 'syscall', 'SYS_capget', 'SYS_capset', 'SYS_close_range',
        'prctl', 'PR_SET_NO_NEW_PRIVS', 'PR_SET_DUMPABLE', 'PR_CAP_AMBIENT',
        'PR_CAP_AMBIENT_CLEAR_ALL', 'close', 'getuid', 'getgid', 'geteuid',
        'setgroups', 'setresuid', 'setresgid', 'getresuid', 'getresgid',
    },
}
MODULE = re.compile(r'((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;')
FORBIDDEN = re.compile(r'\b(?:libbpf_rs|aya|perf_event|perf_event_open|process_vm_readv|ptrace|pidfd_getfd|setns|SCM_RIGHTS|addr2line|framehop|extern)\b|\binclude\s*!')


def check_source(name, source):
    # Preserve quoted URLs rather than mistaking their // for comments.
    source = re.sub(r'("(?:\\.|[^"\\])*")|(/\*.*?\*/|//[^\n]*)',
                    lambda match: match[1] or ' ', source, flags=re.S)
    assert not FORBIDDEN.search(source), f'{name}: privileged implementation/FFI is helper-only'
    allowed = LIBC.get(name, set())
    uses = set(re.findall(r'\blibc\s*::\s*(\w+)', source))
    assert uses <= allowed, f'{name}: unapproved libc APIs: {uses - allowed}'
    # A use alias or local variable must not hide an unreviewed syscall number.
    assert not re.search(r'\buse\b[^;]*\blibc\b', source), f'{name}: libc aliases forbidden'
    calls = re.findall(r'libc\s*::\s*syscall\s*\(\s*([^,\n]+)', source)
    assert len(re.findall(r"\bsyscall\b", source)) == len(calls), f"{name}: syscall aliases forbidden"
    for argument in calls:
        assert argument in {'libc::SYS_capget', 'libc::SYS_capset', 'libc::SYS_close_range'}, f'{name}: unapproved syscall: {argument}'
    return source


def check_tree(root=ROOT):
    seen = set()
    def visit(path, module_dir):
        name = path.relative_to(root).as_posix()
        assert name in ALLOWED, f'{name}: application module outside reviewed allowlist'
        if name in seen:
            return
        seen.add(name)
        source = check_source(name, path.read_text())
        for attributes, module in MODULE.findall(source):
            explicit = re.search(r'#\[path\s*=\s*"([^"]+)"\]', attributes)
            if explicit:
                child = (path.parent / explicit[1]).resolve()
                child_dir = child.parent
            else:
                child = module_dir / (module + '.rs')
                child_dir = module_dir / module
                if not child.exists():
                    child = child_dir / 'mod.rs'
            visit(child, child_dir)
    for entry in ['src/main.rs', 'src/lib.rs']:
        visit(root / entry, root / 'src')
    assert seen == ALLOWED, f'Update the explicit module allowlist: {ALLOWED - seen}'
    # Descriptor passing and fork-based privilege separation are never used.
    for path in (root / 'src').rglob('*.rs'):
        source = re.sub(r'//[^\n]*', '', path.read_text())
        assert not re.search(r'\bSCM_RIGHTS\b|libc::fork\s*\(', source), str(path)


def self_test():
    attacks = ['libc::ptrace(0)', 'use libc::syscall as call;',
               'libc::syscall(libc::SYS_ptrace, 0)', 'use ::libc as l; l::syscall(101, 0);',
               'let call = libc::syscall; call(101, 0);', 'use perf_event::Builder;', 'use libbpf_rs::Map;',
               'extern "C" { fn inspect(); }']
    for source in attacks:
        try:
            check_source('src/http_server/launch.rs', source)
        except AssertionError:
            continue
        raise AssertionError(f'Guard accepted {source}')

    with tempfile.TemporaryDirectory(prefix='procinsh-boundary-') as directory:
        root = Path(directory)
        (root / 'src/privileged_helper').mkdir(parents=True)
        (root / 'src/main.rs').write_text(
            '#[path = "privileged_helper/root.rs"] mod privileged_helper;')
        (root / 'src/privileged_helper/root.rs').write_text('')
        try:
            check_tree(root)
        except AssertionError as error:
            assert 'application module outside reviewed allowlist' in str(error)
        else:
            raise AssertionError('Guard accepted a helper-private module in main')


if __name__ == '__main__':
    self_test()
    check_tree()
    print('Application privilege boundary: passed')
