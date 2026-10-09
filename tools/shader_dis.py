#!/usr/bin/env python3
"""Read the 2018 client's compiled shaders (export_win32/shaders/shaders_sm30).

Usage:
  shader_dis.py list   <shaders_sm30>                       the pool's effects
  shader_dis.py params <shaders_sm30> <effect>              parameters: name <semantic> = default
  shader_dis.py dis    <shaders_sm30> <effect> [technique]  techniques, render states,
                                                            shaders and preshaders

The pool ("PDHS", u32 version, u32 count, then count x (u16 len, name, u32 size,
fx_2_0 effect)) holds Nebula's D3DX effects. Each effect is parsed as Wine's d3dx9
does (parameters, techniques with their Mask annotation, passes, states; the
shaders are its "large objects"); shader bytecode is disassembled (SM2/SM3 tokens)
with the CTAB constant names, and each shader's preshader (PRES: CLIT literals and
FXLC code) is printed, since many constants are computed there (time wrap,
fog reciprocals, displacementFactor x 10...).
The n3 variables reach the shaders through each parameter's semantic
(Intensity0, Amplitude, BumpScale...): `params` lists them.
SEE: docs/fx.md, which these listings were read for.
"""
import struct, sys

OPS = {0: 'nop', 1: 'mov', 2: 'add', 3: 'sub', 4: 'mad', 5: 'mul', 6: 'rcp', 7: 'rsq', 8: 'dp3', 9: 'dp4',
       10: 'min', 11: 'max', 12: 'slt', 13: 'sge', 14: 'exp', 15: 'log', 16: 'lit', 17: 'dst', 18: 'lrp',
       19: 'frc', 20: 'm4x4', 21: 'm4x3', 22: 'm3x4', 23: 'm3x3', 24: 'm3x2', 25: 'call', 26: 'callnz',
       27: 'loop', 28: 'ret', 29: 'endloop', 30: 'label', 31: 'dcl', 32: 'pow', 33: 'crs', 34: 'sgn',
       35: 'abs', 36: 'nrm', 37: 'sincos', 38: 'rep', 39: 'endrep', 40: 'if', 41: 'ifc', 42: 'else',
       43: 'endif', 44: 'break', 45: 'breakc', 46: 'mova', 47: 'defb', 48: 'defi', 64: 'texcoord',
       65: 'texkill', 66: 'texld', 81: 'def', 88: 'cmp', 89: 'bem', 90: 'dp2add', 91: 'dsx', 92: 'dsy',
       93: 'texldd', 94: 'setp', 95: 'texldl', 96: 'breakp'}
NSRC = {'nop': 0, 'mov': 1, 'add': 2, 'sub': 2, 'mad': 3, 'mul': 2, 'rcp': 1, 'rsq': 1, 'dp3': 2, 'dp4': 2,
        'min': 2, 'max': 2, 'slt': 2, 'sge': 2, 'exp': 1, 'log': 1, 'lit': 1, 'dst': 2, 'lrp': 3, 'frc': 1,
        'm4x4': 2, 'm4x3': 2, 'm3x4': 2, 'm3x3': 2, 'm3x2': 2, 'pow': 2, 'crs': 2, 'sgn': 3, 'abs': 1,
        'nrm': 1, 'sincos': 1, 'mova': 1, 'texkill': 0, 'texld': 2, 'cmp': 3, 'dp2add': 3, 'dsx': 1,
        'dsy': 1, 'texldd': 4, 'setp': 2, 'texldl': 2}
NODST = {'if': 1, 'ifc': 2, 'rep': 1, 'loop': 2, 'callnz': 2, 'call': 1, 'breakc': 2, 'breakp': 1, 'label': 1}
VSREG = {0: 'r', 1: 'v', 2: 'c', 3: 'a', 4: 'oPos', 5: 'oD', 6: 'o', 7: 'i', 8: 'oC', 9: 'oDepth', 10: 's',
         14: 'b', 15: 'aL', 17: 'vMisc', 18: 'l', 19: 'p'}
PSREG = {**VSREG, 3: 't'}
USAGE = ['position', 'blendweight', 'blendindices', 'normal', 'psize', 'texcoord', 'tangent', 'binormal',
         'tessfactor', 'positiont', 'color', 'fog', 'depth', 'sample']
SAMP = {2: '2d', 3: 'cube', 4: 'volume'}


def regname(tok, ps):
    t = ((tok >> 28) & 7) | ((tok >> 8) & 0x18)
    n = tok & 0x7ff
    k = (PSREG if ps else VSREG).get(t, f'?{t}')
    if k == 'oPos':
        return ['oPos', 'oFog', 'oPts'][n] if n < 3 else 'oPos?'
    if k == 'vMisc':
        return ['vPos', 'vFace'][n]
    return f'{k}{n}', k, n


def disasm(words, ps, ctab):
    out = []
    i = 1
    while i < len(words):
        tok = words[i]
        op = tok & 0xffff
        if op == 0xfffe:
            i += 1 + ((tok >> 16) & 0x7fff)
            continue
        if op == 0xffff:
            break
        length = (tok >> 24) & 0xf
        args = words[i + 1:i + 1 + length]
        name = OPS.get(op, f'op{op}')
        ctrl = (tok >> 16) & 0xff
        if name == 'ifc' or name == 'breakc' or name == 'setp':
            name += '_' + ['', 'gt', 'eq', 'ge', 'lt', 'ne', 'le'][ctrl & 7]
        if name == 'texld' and ctrl:
            name = ['texld', 'texldp', 'texldb'][ctrl] if ctrl < 3 else name
        if name == 'dcl':
            u = args[0]
            r = fmt_dst(args[1], ps, ctab)
            if regname(args[1], ps)[1] == 's':
                out.append(f'dcl_{SAMP.get((u >> 27) & 0xf, "?")} {r}')
            else:
                out.append(f'dcl_{USAGE[u & 0x1f]}{(u >> 16) & 0xf} {r}')
        elif name == 'def':
            f = struct.unpack('<4f', struct.pack('<4I', *args[1:5]))
            out.append(f'def {fmt_dst(args[0], ps, None)}, ' + ', '.join(f'{x:g}' for x in f))
        elif name in ('defi',):
            out.append(f'defi {fmt_dst(args[0], ps, None)}, ' + ', '.join(str(struct.unpack("<i", struct.pack("<I", x))[0]) for x in args[1:5]))
        elif name == 'defb':
            out.append(f'defb {fmt_dst(args[0], ps, None)}, {args[1]}')
        elif name.split('_')[0] in NODST or name in ('else', 'endif', 'endrep', 'endloop', 'ret', 'break'):
            out.append(name + ' ' + ', '.join(fmt_src(a, ps, ctab) for a in args))
        else:
            parts = [fmt_dst(args[0], ps, ctab)] if args else []
            parts += [fmt_src(a, ps, ctab) for a in args[1:]]
            pred = '(p0) ' if tok & (1 << 28) else ''
            out.append(pred + name + ' ' + ', '.join(parts))
        i += 1 + length
    return out


def cname(k, n, ctab):
    if ctab is None:
        return ''
    for (rs, ri, cnt, nm) in ctab:
        if rs == k and ri <= n < ri + cnt:
            return f'{{{nm}' + (f'[{n - ri}]' if cnt > 1 else '') + '}'
    return ''


def fmt_dst(tok, ps, ctab):
    r = regname(tok, ps)
    if isinstance(r, str):
        s = r
    else:
        s = r[0] + cname({'c': 2, 'i': 1, 'b': 0, 's': 3}.get(r[1], -1), r[2], ctab)
    m = (tok >> 16) & 0xf
    if m != 0xf:
        s += '.' + ''.join(c for b, c in zip(range(4), 'xyzw') if m & (1 << b))
    mod = (tok >> 20) & 0xf
    if mod & 1:
        s = s.replace(' ', '') + '_sat'
    return s


def fmt_src(tok, ps, ctab):
    r = regname(tok, ps)
    if isinstance(r, str):
        s = r
    else:
        s = r[0] + cname({'c': 2, 'i': 1, 'b': 0, 's': 3}.get(r[1], -1), r[2], ctab)
    if tok & (1 << 13):
        s += '[a0/aL]'
    sw = (tok >> 16) & 0xff
    comps = ''.join('xyzw'[(sw >> (2 * k)) & 3] for k in range(4))
    if comps != 'xyzw':
        if len(set(comps)) == 1:
            comps = comps[0]
        s += '.' + comps
    mod = (tok >> 24) & 0xf
    s = {0: s, 1: '-' + s, 2: s + '_bias', 3: '-' + s + '_bias', 4: s + '_bx2', 5: '-' + s + '_bx2',
         6: '1-' + s, 7: s + '_x2', 8: '-' + s + '_x2', 9: s + '_dz', 10: s + '_dw', 11: 'abs(' + s + ')',
         12: '-abs(' + s + ')', 13: '!' + s}.get(mod, s)
    return s


def parse_ctab(words):
    i = 1
    while i < len(words):
        tok = words[i]
        if tok & 0xffff != 0xfffe:
            return []
        n = (tok >> 16) & 0x7fff
        blob = struct.pack(f'<{n}I', *words[i + 1:i + 1 + n])
        if blob[:4] == b'CTAB':
            b = blob[4:]
            _sz, _creator, _ver, count, cinfo = struct.unpack_from('<5I', b, 0)
            out = []
            for k in range(count):
                name, rs, ri, rc, _res, _ti, _dv = struct.unpack_from('<IHHHHII', b, cinfo + 20 * k)
                nm = b[name:b.index(b'\0', name)].decode()
                out.append((rs, ri, rc, nm))
            return out
        i += 1 + n
    return []


def shader(blob):
    words = list(struct.unpack(f'<{len(blob) // 4}I', blob[:len(blob) // 4 * 4]))
    ver = words[0]
    ps = (ver >> 16) == 0xffff
    ctab = parse_ctab(words)
    kind = ('ps' if ps else 'vs') + f'_{(ver >> 8) & 0xff}_{ver & 0xff}'
    return kind, ctab, disasm(words, ps, ctab)


STATES = {92: 'VertexShader', 93: 'PixelShader'}


def parse_effect(d):
    base_size = struct.unpack_from('<I', d, 4)[0]
    base = d[8:8 + base_size]
    p = 8 + base_size

    def u32():
        nonlocal p
        v = struct.unpack_from('<I', d, p)[0]
        p += 4
        return v

    def name_at(off):
        n = struct.unpack_from('<I', base, off)[0]
        return base[off + 4:off + 4 + n].rstrip(b'\0').decode('latin1')

    nparams, ntechs, _unk, _nobjs = u32(), u32(), u32(), u32()
    params = []
    for _ in range(nparams):
        tdef, val, flags, nann = u32(), u32(), u32(), u32()
        anns = [(u32(), u32()) for _ in range(nann)]
        pname = name_at(struct.unpack_from('<I', base, tdef + 8)[0])
        params.append((pname, tdef, val, anns))
    techs = []
    for _ in range(ntechs):
        tname, nann, npass = u32(), u32(), u32()
        anns = [(u32(), u32()) for _ in range(nann)]
        passes = []
        for _ in range(npass):
            pname, pann, nstate = u32(), u32(), u32()
            _ = [(u32(), u32()) for _ in range(pann)]
            states = [(u32(), u32(), u32(), u32()) for _ in range(nstate)]
            passes.append({'name': name_at(pname) if pname else '', 'states': states, 'shaders': []})
        techs.append({'name': name_at(tname), 'anns': anns, 'passes': passes})
    nsmall, nlarge = u32(), u32()
    small = {}
    for _ in range(nsmall):
        idx, size = u32(), u32()
        small[idx] = d[p:p + size].rstrip(b'\0').decode('latin1')
        p += (size + 3) & ~3
    for _ in range(nlarge):
        ti, idx, _el, si, usage, size = u32(), u32(), u32(), u32(), u32(), u32()
        blob = d[p:p + size]
        p += (size + 3) & ~3
        if ti != 0xffffffff and ti < len(techs):
            ps = techs[ti]['passes'][idx]
            ps['shaders'].append((si, usage, blob))
    # Mask annotations: value is an object id.
    for t in techs:
        t['mask'] = ''
        for (tdef, val) in t['anns']:
            oid = struct.unpack_from('<I', base, val)[0]
            if oid in small:
                t['mask'] = small[oid]
    return params, techs, base


def state_value(base, st):
    op, _idx, tdef, val = st
    try:
        return struct.unpack_from('<I', base, val)[0]
    except struct.error:
        return None


RS = {0: 'ZEnable', 1: 'FillMode', 2: 'ShadeMode', 3: 'ZWriteEnable', 4: 'AlphaTestEnable', 5: 'LastPixel',
      6: 'SrcBlend', 7: 'DestBlend', 8: 'CullMode', 9: 'ZFunc', 10: 'AlphaRef', 11: 'AlphaFunc',
      12: 'DitherEnable', 13: 'AlphaBlendEnable', 14: 'FogEnable', 15: 'SpecularEnable',
      23: 'StencilEnable', 24: 'StencilFail', 25: 'StencilZFail', 26: 'StencilPass', 27: 'StencilFunc',
      28: 'StencilRef', 29: 'StencilMask', 30: 'StencilWriteMask', 38: 'ColorWriteEnable', 39: 'BlendOp',
      48: 'SeparateAlphaBlendEnable', 49: 'SrcBlendAlpha', 50: 'DestBlendAlpha', 51: 'BlendOpAlpha',
      45: 'DepthBias', 42: 'SlopeScaleDepthBias'}


PRES_OPS = {0x100: 'mov', 0x101: 'neg', 0x103: 'rcp', 0x104: 'frc', 0x105: 'exp', 0x106: 'log', 0x107: 'rsq',
       0x108: 'sin', 0x109: 'cos', 0x10a: 'asin', 0x10b: 'acos', 0x10c: 'atan', 0x200: 'min', 0x201: 'max',
       0x202: 'lt', 0x203: 'ge', 0x204: 'add', 0x205: 'mul', 0x206: 'atan2', 0x208: 'div', 0x300: 'cmp',
       0x301: 'movc', 0x500: 'dot', 0x502: 'noise', 0x700: 'min', 0x701: 'max', 0x702: 'lt', 0x703: 'ge',
       0x704: 'add', 0x705: 'mul', 0x706: 'atan2', 0x708: 'div'}

def comments(words, start=1):
    i = start
    while i < len(words):
        t = words[i]
        if t & 0xffff != 0xfffe:
            break
        n = (t >> 16) & 0x7fff
        yield words[i + 1:i + 1 + n]
        i += 1 + n

def pres(words):
    for c in comments(words):
        if c and c[0] == 0x53455250:  # PRES
            inner = c[1:]
            ctab, lits, code = [], [], []
            for cc in comments(inner, 1):
                tag = cc[0]
                body = cc[1:]
                if tag == 0x42415443:  # CTAB
                    ctab = parse_ctab([0, (len(cc) << 16) | 0xfffe] + list(cc))
                elif tag == 0x54494c43:  # CLIT
                    n = body[0]
                    lits = list(struct.unpack('<%dd' % n, struct.pack('<%dI' % (2 * n), *body[1:1 + 2 * n])))
                elif tag == 0x434c5846:  # FXLC
                    code = body
            return ctab, lits, code
    return None

def arg(p, code, ctab, lits, ncomp):
    flags = code[p]; p += 1
    idx = ''
    if flags:
        idx = f'[t{code[p]}:{code[p+1]}]'; p += 2
    table, off = code[p], code[p + 1]; p += 2
    reg, comp = divmod(off, 4)
    sw = 'xyzw'[comp:comp + ncomp] if comp + ncomp <= 4 else f'+{comp}'
    if table == 1:
        s = 'lit(' + ','.join(f'{lits[off + k]:g}' if off + k < len(lits) else '?' for k in range(ncomp)) + ')'
    elif table == 2:
        nm = next((n + (f'[{reg - ri}]' if c > 1 else '') for rs, ri, c, n in ctab if ri <= reg < ri + c), f'?{reg}')
        s = f'{nm}.{sw}'
    elif table == 4:
        s = f'c{reg}.{sw}'
    elif table == 7:
        s = f't{reg}.{sw}'
    else:
        s = f'T{table}:{off}'
    return p, s + idx

def dump(code, ctab, lits):
    n = code[0]; p = 1
    out = []
    for _ in range(n):
        ins = code[p]; p += 1
        op = (ins >> 20) & 0x7ff
        ncomp = ins & 0xffff
        scalar = ins >> 31
        nin = code[p]; p += 1
        ins_args = []
        for k in range(nin):
            p, s = arg(p, code, ctab, lits, 1 if (scalar and k == 0) else ncomp)
            ins_args.append(s)
        p, o = arg(p, code, ctab, lits, ncomp)
        out.append(f'{PRES_OPS.get(op, hex(op))} {o} = ' + ', '.join(ins_args))
    return out



def pool(path):
    """{effect name: fx_2_0 bytes} of a shader pool."""
    d = open(path, 'rb').read()
    if d[:4] != b'PDHS':
        raise SystemExit(f'{path}: not a Nebula shader pool')
    n = struct.unpack_from('<I', d, 8)[0]
    i, out = 12, {}
    for _ in range(n):
        ln = struct.unpack_from('<H', d, i)[0]
        name = d[i + 2:i + 2 + ln].decode()
        i += 2 + ln
        size = struct.unpack_from('<I', d, i)[0]
        i += 4
        out[name] = d[i:i + size]
        i += size
    return out


def params_of(d):
    params, techs, base = parse_effect(d)

    def s(off):
        if not off:
            return ''
        n = struct.unpack_from('<I', base, off)[0]
        return base[off + 4:off + 4 + n].rstrip(b'\0').decode('latin1')
    out = []
    for (name, tdef, val, anns) in params:
        typ, cls, _noff, soff = struct.unpack_from('<IIII', base, tdef)
        v = ''
        if cls in (0, 1) and typ in (1, 2, 3):
            rows, cols = struct.unpack_from('<II', base, tdef + 20)
            n = rows * cols
            v = ' = ' + ','.join(f'{x:g}' for x in struct.unpack_from('<%d%s' % (n, 'f' if typ == 3 else 'i'), base, val))
        out.append(f'{name} <{s(soff)}>{v}')
    return out


def dis_effect(d, want=None):
    params, techs, base = parse_effect(d)
    seen = {}
    for t in techs:
        if want and want != t['name']:
            continue
        print(f"=== technique {t['name']}  mask={t['mask']!r}")
        for ps in t['passes']:
            sts = [f'{RS[st[0]]}={state_value(base, st)}' for st in ps['states'] if st[0] in RS]
            print('  pass', ps['name'], ' '.join(sts))
            for (_si, _usage, blob) in ps['shaders']:
                if len(blob) < 8 or (struct.unpack_from('<I', blob)[0] >> 16) not in (0xfffe, 0xffff):
                    continue
                kind, ctab, lines = shader(blob)
                if blob in seen:
                    print(f'    {kind}: same as {seen[blob]}')
                    continue
                seen[blob] = f"{t['name']}/{kind}"
                print(f'    {kind}: consts ' + ', '.join(f"{['b', 'i', 'c', 's'][r]}{i}:{n}" for r, i, c, n in ctab))
                for ln in lines:
                    print('      ' + ln)
                words = list(struct.unpack('<%dI' % (len(blob) // 4), blob[:len(blob) // 4 * 4]))
                r = pres(words)
                if r:
                    print('    preshader:')
                    for ln in dump(r[2], r[0], r[1]):
                        print('      ' + ln)


if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(__doc__)
        raise SystemExit(2)
    cmd, effects = sys.argv[1], pool(sys.argv[2])
    if cmd == 'list':
        for name, blob in effects.items():
            print(f'{name}\t{len(blob)}')
    elif cmd == 'params':
        print('\n'.join(params_of(effects[sys.argv[3]])))
    elif cmd == 'dis':
        dis_effect(effects[sys.argv[3]], sys.argv[4] if len(sys.argv) > 4 else None)
    else:
        raise SystemExit(f'unknown command {cmd}')
