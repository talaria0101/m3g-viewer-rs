#!/usr/bin/env python3
"""Writes small .m3g (JSR-184) scenes that exercise renderer features the three
shipped assets do not: lights, fog, flat shading, two-sided lighting, texture
formats and functions, blending, alpha test, vertex colours, animation kinds.

Usage: m3gen.py OUTDIR
Standard library only. The files are test inputs for compare_scenes.sh.
"""
import math
import os
import struct
import sys
import zlib

MAGIC = bytes([0xAB, 0x4A, 0x53, 0x52, 0x31, 0x38, 0x34, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A])


def adler32(data):
    return zlib.adler32(data) & 0xFFFFFFFF


class W:
    """Little-endian byte writer."""

    def __init__(self):
        self.b = bytearray()

    def u8(self, v): self.b += struct.pack('<B', v & 255); return self
    def u16(self, v): self.b += struct.pack('<H', v & 0xFFFF); return self
    def i32(self, v): self.b += struct.pack('<i', v); return self
    def u32(self, v): self.b += struct.pack('<I', v & 0xFFFFFFFF); return self
    def f32(self, *vs):
        for v in vs: self.b += struct.pack('<f', v)
        return self
    def bit(self, v): return self.u8(1 if v else 0)
    def rgb(self, c): return self.u8(c >> 16).u8(c >> 8).u8(c)
    def rgba(self, c): return self.u8(c >> 16).u8(c >> 8).u8(c).u8(c >> 24)
    def raw(self, bs): self.b += bs; return self
    def string(self, s): self.b += s.encode() + b'\0'; return self


class Builder:
    def __init__(self):
        self.objs = []  # (type, bytes)

    def add(self, typ, w):
        self.objs.append((typ, bytes(w.b)))
        return len(self.objs) + 1  # reference index (1 is the header's slot, objects start at 2)

    # ---- generic parts
    @staticmethod
    def o3d(w, tracks=()):
        w.u32(0).u32(len(tracks))
        for t in tracks: w.u32(t)
        w.u32(0)

    @staticmethod
    def transformable(w, t=None, s=None, rot=None, matrix=None):
        if t or s or rot:
            w.bit(True)
            w.f32(*(t or (0, 0, 0))).f32(*(s or (1, 1, 1)))
            angle, ax, ay, az = rot or (0, 0, 1, 0)
            w.f32(angle, ax, ay, az)
        else:
            w.bit(False)
        if matrix:
            w.bit(True); w.f32(*matrix)
        else:
            w.bit(False)

    def node(self, w, tracks=(), t=None, s=None, rot=None, matrix=None, alpha=255, render=True, scope=-1):
        self.o3d(w, tracks)
        self.transformable(w, t, s, rot, matrix)
        w.bit(render).bit(True).u8(alpha).u32(scope).bit(False)

    # ---- objects
    def vertex_array(self, comps, size, data, encoding=0):
        w = W()
        self.o3d(w)
        n = len(data) // comps
        w.u8(size).u8(comps).u8(encoding).u16(n)
        fmt = '<b' if size == 1 else '<h'
        for v in data: w.raw(struct.pack(fmt, v))
        return self.add(20, w)

    def color_array(self, comps, data):
        w = W()
        self.o3d(w)
        w.u8(1).u8(comps).u8(0).u16(len(data) // comps)
        for v in data: w.u8(v)
        return self.add(20, w)

    def vertex_buffer(self, pos, pscale, pbias, normals=0, colors=0, uvs=(), default=0xFFFFFFFF, tracks=()):
        w = W()
        self.o3d(w, tracks)
        w.rgba(default).u32(pos).f32(*pbias).f32(pscale).u32(normals).u32(colors).u32(len(uvs))
        for ref, scale, bias in uvs:
            w.u32(ref).f32(*bias).f32(scale)
        return self.add(21, w)

    def strips(self, lengths, indices):
        w = W()
        self.o3d(w)
        w.u8(128).u32(len(indices))
        for i in indices: w.u32(i)
        w.u32(len(lengths))
        for l in lengths: w.u32(l)
        return self.add(11, w)

    def material(self, ambient=0x333333, diffuse=0xFFCCCCCC, emissive=0, specular=0, shininess=0.0, track=False, tracks=()):
        w = W()
        self.o3d(w, tracks)
        w.rgb(ambient).rgba(diffuse).rgb(emissive).rgb(specular).f32(shininess).bit(track)
        return self.add(13, w)

    def polygon_mode(self, cull=160, shade=165, winding=168, two=False, local=False, persp=True):
        w = W()
        self.o3d(w)
        w.u8(cull).u8(shade).u8(winding).bit(two).bit(local).bit(persp)
        return self.add(8, w)

    def compositing(self, blend=68, thr=0, dt=True, dw=True, cw=True, aw=True, off=(0.0, 0.0)):
        w = W()
        self.o3d(w)
        w.bit(dt).bit(dw).bit(cw).bit(aw).u8(blend).u8(thr).f32(*off)
        return self.add(6, w)

    def fog(self, color, mode, a, b=0.0):
        w = W()
        self.o3d(w)
        w.rgb(color).u8(mode)
        if mode == 80: w.f32(a)
        else: w.f32(a, b)
        return self.add(7, w)

    def image(self, fmt, width, height, pixels):
        w = W()
        self.o3d(w)
        w.u8(fmt).bit(False).u32(width).u32(height).u32(0).u32(len(pixels)).raw(bytes(pixels))
        return self.add(10, w)

    def texture(self, image, blend=227, wrap=(241, 241), filt=(208, 210), color=0, t=None, s=None, rot=None, tracks=()):
        w = W()
        self.o3d(w, tracks)
        self.transformable(w, t, s, rot)
        w.u32(image).rgb(color).u8(blend).u8(wrap[0]).u8(wrap[1]).u8(filt[0]).u8(filt[1])
        return self.add(17, w)

    def appearance(self, layer=0, comp=0, fog=0, poly=0, mat=0, textures=()):
        w = W()
        self.o3d(w)
        w.u8(layer).u32(comp).u32(fog).u32(poly).u32(mat).u32(len(textures))
        for t in textures: w.u32(t)
        return self.add(3, w)

    def mesh(self, vb, subs, tracks=(), **kw):
        w = W()
        self.node(w, tracks, **kw)
        w.u32(vb).u32(len(subs))
        for ib, ap in subs: w.u32(ib).u32(ap)
        return self.add(14, w)

    def group(self, children, tracks=(), **kw):
        w = W()
        self.node(w, tracks, **kw)
        w.u32(len(children))
        for c in children: w.u32(c)
        return self.add(9, w)

    def camera_persp(self, fovy=45.0, aspect=1.333, near=0.1, far=50.0, **kw):
        w = W()
        self.node(w, **kw)
        w.u8(50).f32(fovy, aspect, near, far)
        return self.add(5, w)

    def camera_parallel(self, height=4.0, aspect=1.333, near=0.1, far=50.0, **kw):
        w = W()
        self.node(w, **kw)
        w.u8(49).f32(height, aspect, near, far)
        return self.add(5, w)

    def light(self, mode, color=0xFFFFFF, intensity=1.0, atten=(1.0, 0.0, 0.0), spot=(45.0, 0.0), tracks=(), **kw):
        w = W()
        self.node(w, tracks, **kw)
        w.f32(*atten).rgb(color).u8(mode).f32(intensity).f32(*spot)
        return self.add(12, w)

    def background(self, color, image=0, crop=(0, 0, 0, 0), tracks=()):
        w = W()
        self.o3d(w, tracks)
        w.rgba(color).u32(image).u8(32).u8(32).i32(crop[0]).i32(crop[1]).i32(crop[2]).i32(crop[3]).bit(True).bit(True)
        return self.add(4, w)

    def world(self, children, camera=0, background=0, tracks=()):
        w = W()
        self.node(w, tracks)
        w.u32(len(children))
        for c in children: w.u32(c)
        w.u32(camera).u32(background)
        return self.add(22, w)

    def keyframes(self, interp, repeat, duration, comps, keys, encoding=0, bias=None, scale=None, valid=None):
        w = W()
        self.o3d(w)
        first, last = valid or (0, len(keys) - 1)
        w.u8(interp).u8(repeat).u8(encoding).u32(duration).u32(first).u32(last).u32(comps).u32(len(keys))
        if encoding == 0:
            for time, vals in keys:
                w.i32(time).f32(*vals)
        else:
            w.f32(*bias).f32(*scale)
            for time, vals in keys:
                w.i32(time)
                for c in range(comps):
                    norm = (vals[c] - bias[c]) / scale[c]
                    if encoding == 1: w.u8(int(round(norm * 255)))
                    else: w.u16(int(round(norm * 65535)))
        return self.add(19, w)

    def controller(self, speed=1.0, weight=1.0, active=(0, 0), ref_seq=0.0, ref_world=0):
        w = W()
        self.o3d(w)
        w.f32(speed, weight).i32(active[0]).i32(active[1]).f32(ref_seq).i32(ref_world)
        return self.add(1, w)

    def track(self, seq, ctrl, prop):
        w = W()
        self.o3d(w)
        w.u32(seq).u32(ctrl).u32(prop)
        return self.add(2, w)

    def morphing_mesh(self, vb, targets, subs, tracks=(), weights=None, **kw):
        w = W()
        self.node(w, tracks, **kw)
        w.u32(vb).u32(len(subs))
        for ib, ap in subs: w.u32(ib).u32(ap)
        w.u32(len(targets))
        for i, t in enumerate(targets): w.u32(t).f32((weights or [0.0] * len(targets))[i])
        return self.add(15, w)

    def skinned_mesh(self, vb, subs, skeleton, bones, tracks=(), **kw):
        w = W()
        self.node(w, tracks, **kw)
        w.u32(vb).u32(len(subs))
        for ib, ap in subs: w.u32(ib).u32(ap)
        w.u32(skeleton).u32(len(bones))
        for bone, first, count, weight in bones: w.u32(bone).u32(first).u32(count).i32(weight)
        return self.add(16, w)

    def save(self, path, roots_note="m3gen"):
        header = W().u8(1).u8(0).bit(False).u32(0).u32(0).string(roots_note)
        sections = [self.section([(0, bytes(header.b))]), self.section(self.objs)]
        body = b''.join(sections)
        total = len(MAGIC) + len(body)
        # patch totalFileSize in the header (first section, after version(2)+bit(1))
        data = bytearray(MAGIC + body)
        # header object: type(1) len(4) | version 2 | ext 1 | total 4 ...; section header is 9 bytes
        off = len(MAGIC) + 9 + 5 + 3
        struct.pack_into('<I', data, off, total)
        # recompute section 0 checksum after patching
        sec0_len = len(sections[0])
        start = len(MAGIC)
        struct.pack_into('<I', data, start + sec0_len - 4, adler32(bytes(data[start:start + sec0_len - 4])))
        with open(path, 'wb') as f: f.write(data)

    @staticmethod
    def section(objs):
        payload = b''.join(struct.pack('<BI', t, len(d)) + d for t, d in objs)
        head = struct.pack('<BII', 0, len(payload) + 13, len(payload))
        body = head + payload
        return body + struct.pack('<I', adler32(body))


# ---------------------------------------------------------------- geometry

def cube(b, size=1.0):
    """24-vertex cube with per-face normals, uv per face, as shorts. Returns (pos, normals, uv, strips)."""
    faces = [
        ((0, 0, 1), [(-1, -1, 1), (1, -1, 1), (-1, 1, 1), (1, 1, 1)]),
        ((0, 0, -1), [(1, -1, -1), (-1, -1, -1), (1, 1, -1), (-1, 1, -1)]),
        ((1, 0, 0), [(1, -1, 1), (1, -1, -1), (1, 1, 1), (1, 1, -1)]),
        ((-1, 0, 0), [(-1, -1, -1), (-1, -1, 1), (-1, 1, -1), (-1, 1, 1)]),
        ((0, 1, 0), [(-1, 1, 1), (1, 1, 1), (-1, 1, -1), (1, 1, -1)]),
        ((0, -1, 0), [(-1, -1, -1), (1, -1, -1), (-1, -1, 1), (1, -1, 1)]),
    ]
    pos, nrm, uv, idx, lens = [], [], [], [], []
    for n, quad in faces:
        base = len(pos) // 3
        for k, (x, y, z) in enumerate(quad):
            pos += [x * 1000, y * 1000, z * 1000]
            nrm += [v * 127 for v in n]
            uv += [(k & 1) * 1000, (1 - (k >> 1)) * 1000]
        idx += [base, base + 1, base + 2, base + 3]
        lens.append(4)
    return pos, nrm, uv, idx, lens


def checker(n, a, b):
    out = []
    for y in range(n):
        for x in range(n):
            out += a if ((x // 2 + y // 2) & 1) == 0 else b
    return out


def gradient_rgba(n):
    out = []
    for y in range(n):
        for x in range(n):
            out += [x * 255 // (n - 1), y * 255 // (n - 1), 128, (x + y) * 255 // (2 * (n - 1))]
    return out


def textured_cube_scene(b, material, comp=0, poly=0, fog=0, textures=(), lights=(), colors=None, normals_bytes=False,
                        default_color=0xFFFFFFFF, mesh_kw=None, extra_children=(), camera=True, background=0, use_normals=True):
    pos, nrm, uv, idx, lens = cube(b)
    if normals_bytes:
        nrm = [max(-127, min(127, v)) for v in nrm]
        nref = b.vertex_array(3, 1, nrm) if use_normals else 0
    else:
        nrm = [v * 257 for v in nrm]
        nref = b.vertex_array(3, 2, nrm) if use_normals else 0
    pref = b.vertex_array(3, 2, pos)
    uref = b.vertex_array(2, 2, uv)
    cref = 0
    if colors is not None:
        cref = b.color_array(colors[0], colors[1])
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, cref, [(uref, 0.001, (0, 0, 0))] if textures else [], default=default_color)
    ib = b.strips(lens, idx)
    ap = b.appearance(comp=comp, fog=fog, poly=poly, mat=material, textures=textures)
    m = b.mesh(vb, [(ib, ap)], **(mesh_kw or {}))
    cam = b.camera_persp(t=(0, 0, 6)) if camera else 0
    kids = list(lights) + [m] + list(extra_children) + ([cam] if camera else [])
    return b.world(kids, camera=cam, background=background)


# ---------------------------------------------------------------- scenes

def scene_lights(out):
    b = Builder()
    mat = b.material(ambient=0x303030, diffuse=0xFFB0C0E0, emissive=0x101010, specular=0xFFFFFF, shininess=24.0)
    amb = b.light(128, color=0x202020)
    dirl = b.light(129, color=0xFFEEDD, intensity=0.8, rot=(-30, 1, 1, 0))
    omni = b.light(130, color=0x4040FF, intensity=1.5, atten=(1.0, 0.1, 0.05), t=(-3, 2, 3))
    spot = b.light(131, color=0xFF4040, intensity=2.0, atten=(1.0, 0.0, 0.02), spot=(35.0, 8.0), t=(3, 3, 4), rot=(-30, 1, -0.5, 0))
    world = textured_cube_scene(b, mat, lights=[amb, dirl, omni, spot], mesh_kw=dict(rot=(30, 0.3, 1, 0)))
    b.save(os.path.join(out, 'lights.m3g'))


def scene_lights_local_viewer(out):
    b = Builder()
    mat = b.material(ambient=0x202020, diffuse=0xFF909090, specular=0xFFFFFF, shininess=64.0)
    poly = b.polygon_mode(local=True)
    omni = b.light(130, color=0xFFFFFF, intensity=1.2, t=(2, 2, 4))
    textured_cube_scene(b, mat, poly=poly, lights=[omni], normals_bytes=True)
    b.save(os.path.join(out, 'lights_local_viewer.m3g'))


def scene_flat_twosided(out):
    b = Builder()
    mat = b.material(ambient=0x202020, diffuse=0xFFFFA040, specular=0xFF404040, shininess=10.0)
    poly = b.polygon_mode(cull=162, shade=164, two=True)
    dirl = b.light(129, color=0xFFFFFF, rot=(-45, 0, 1, 0))
    omni = b.light(130, color=0x6060FF, atten=(1.0, 0.0, 0.1), t=(0, 0, 3))
    textured_cube_scene(b, mat, poly=poly, lights=[dirl, omni], mesh_kw=dict(rot=(25, 1, 1, 0), s=(1, 1, 0.5)))
    b.save(os.path.join(out, 'flat_twosided.m3g'))


def scene_smooth_twosided(out):
    b = Builder()
    mat = b.material(ambient=0x202020, diffuse=0xFFA0C040, specular=0xFFFFFFFF, shininess=30.0)
    poly = b.polygon_mode(cull=162, two=True, winding=169)
    dirl = b.light(129, color=0xFFFFFF, rot=(-60, 1, 0, 0))
    textured_cube_scene(b, mat, poly=poly, lights=[dirl], mesh_kw=dict(rot=(25, 1, 1, 0)))
    b.save(os.path.join(out, 'smooth_twosided_cw.m3g'))


def scene_cull_front(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    poly = b.polygon_mode(cull=161)
    textured_cube_scene(b, mat, poly=poly, mesh_kw=dict(rot=(25, 1, 1, 0)))
    b.save(os.path.join(out, 'cull_front.m3g'))


def scene_fog(out):
    for name, make_fog in (('fog_linear', lambda b: b.fog(0x8090A0, 81, 4.0, 9.0)),
                           ('fog_exp', lambda b: b.fog(0xA08060, 80, 0.15))):
        b = Builder()
        mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
        img = b.image(99, 8, 8, checker(8, [255, 255, 255], [40, 80, 200]))
        tex = b.texture(img)
        textured_cube_scene(b, mat, fog=make_fog(b), textures=[tex], mesh_kw=dict(rot=(25, 1, 1, 0)))
        b.save(os.path.join(out, name + '.m3g'))


def scene_tex_formats(out):
    funcs = {'replace': 228, 'modulate': 227, 'decal': 226, 'blend': 225, 'add': 224}
    formats = {
        'rgb': (99, 3, lambda n: checker(n, [220, 60, 60], [60, 220, 60])),
        'rgba': (100, 4, lambda n: gradient_rgba(n)),
        'lum': (97, 1, lambda n: [ (x * 255) // (n * n) for x in range(n * n)]),
        'alpha': (96, 1, lambda n: [ ((x % n) * 255) // (n - 1) for x in range(n * n)]),
        'lumalpha': (98, 2, lambda n: sum([[ (x * 255) // (n * n), ((x // n) * 255) // (n - 1)] for x in range(n * n)], [])),
    }
    for fname, (fmt, _, gen) in formats.items():
        for func_name, func in funcs.items():
            b = Builder()
            mat = b.material(ambient=0x909090, diffuse=0xFFFFC080)
            img = b.image(fmt, 8, 8, gen(8))
            tex = b.texture(img, blend=func, color=0x2060C0)
            comp = b.compositing(blend=64)
            textured_cube_scene(b, mat, comp=comp, textures=[tex], mesh_kw=dict(rot=(25, 1, 1, 0)))
            b.save(os.path.join(out, f'tex_{fname}_{func_name}.m3g'))


def scene_tex_wrap_filter_transform(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    img = b.image(100, 8, 8, gradient_rgba(8))
    tex = b.texture(img, wrap=(240, 241), filt=(208, 209), t=(0.3, 0.1, 0), s=(1.7, 1.2, 1), rot=(20, 0, 0, 1))
    textured_cube_scene(b, mat, textures=[tex], mesh_kw=dict(rot=(25, 1, 1, 0)))
    b.save(os.path.join(out, 'tex_wrap_filter_xform.m3g'))


def scene_two_units(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    img0 = b.image(99, 8, 8, checker(8, [255, 255, 255], [90, 90, 255]))
    img1 = b.image(100, 8, 8, gradient_rgba(8))
    t0 = b.texture(img0, blend=227)
    t1 = b.texture(img1, blend=224, s=(2, 2, 1))
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    uref = b.vertex_array(2, 2, uv)
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0)), (uref, 0.002, (0, 0.1, 0))])
    ib = b.strips(lens, idx)
    ap = b.appearance(mat=mat, textures=[t0, t1])
    m = b.mesh(vb, [(ib, ap)], rot=(25, 1, 1, 0))
    cam = b.camera_persp(t=(0, 0, 6))
    b.world([m, cam], camera=cam)
    b.save(os.path.join(out, 'two_units.m3g'))


def scene_blend_modes(out):
    for name, blend in (('alpha', 64), ('alpha_add', 65), ('modulate', 66), ('modulate_x2', 67)):
        b = Builder()
        mat = b.material(ambient=0xFFFFFF, diffuse=0xA0FFFFFF)
        img = b.image(100, 8, 8, gradient_rgba(8))
        tex = b.texture(img)
        back = b.compositing(blend=68)
        front = b.compositing(blend=blend, dw=False)
        pos, nrm, uv, idx, lens = cube(b)
        pref = b.vertex_array(3, 2, pos)
        nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
        uref = b.vertex_array(2, 2, uv)
        vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0))])
        ib = b.strips(lens, idx)
        ap_back = b.appearance(comp=back, mat=mat, textures=[tex])
        ap_front = b.appearance(comp=front, mat=mat, textures=[tex])
        m1 = b.mesh(vb, [(ib, ap_back)], rot=(25, 1, 1, 0), s=(1.6, 1.6, 1.6))
        m2 = b.mesh(vb, [(ib, ap_front)], rot=(-20, 1, 0.3, 0), s=(0.8, 0.8, 0.8), alpha=200)
        cam = b.camera_persp(t=(0, 0, 6))
        b.world([m1, m2, cam], camera=cam)
        b.save(os.path.join(out, f'blend_{name}.m3g'))


def scene_alpha_test(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    img = b.image(100, 8, 8, gradient_rgba(8))
    tex = b.texture(img, blend=228)
    comp = b.compositing(blend=64, thr=128)
    textured_cube_scene(b, mat, comp=comp, textures=[tex], mesh_kw=dict(rot=(25, 1, 1, 0)))
    b.save(os.path.join(out, 'alpha_test.m3g'))


def scene_depth_flags(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    img = b.image(99, 8, 8, checker(8, [255, 255, 255], [200, 60, 60]))
    tex = b.texture(img)
    nodepth = b.compositing(blend=68, dt=False, dw=True)
    nowrite = b.compositing(blend=68, dt=True, dw=False)
    mask = b.compositing(blend=68, cw=False)
    offset = b.compositing(blend=68, off=(1.0, 2.0))
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    uref = b.vertex_array(2, 2, uv)
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0))])
    ib = b.strips(lens, idx)
    meshes = []
    for i, comp in enumerate((nodepth, nowrite, mask, offset)):
        ap = b.appearance(layer=i % 2, comp=comp, mat=mat, textures=[tex])
        meshes.append(b.mesh(vb, [(ib, ap)], t=(-2.4 + 1.6 * i, 0, 0.3 * i), rot=(25 + 10 * i, 1, 1, 0), s=(0.6, 0.6, 0.6)))
    cam = b.camera_persp(t=(0, 0, 8))
    b.world(meshes + [cam], camera=cam)
    b.save(os.path.join(out, 'depth_flags.m3g'))


def scene_vertex_colors(out):
    cols3 = []
    cols4 = []
    for i in range(24):
        cols3 += [(i * 37) & 255, (i * 91) & 255, (i * 53) & 255]
        cols4 += [(i * 37) & 255, (i * 91) & 255, (i * 53) & 255, 255 - i * 8]
    for name, colors, mat_kw, comp in (
        ('vcol3_lit_track', (3, cols3), dict(track=True), 0),
        ('vcol4_lit_track_alpha', (4, cols4), dict(track=True), 64),
        ('vcol3_unlit', (3, cols3), None, 0),
        ('vcol4_unlit_alpha', (4, cols4), None, 64),
    ):
        b = Builder()
        mat = b.material(ambient=0x404040, diffuse=0xFFFFFFFF, **mat_kw) if mat_kw is not None else 0
        lights = [b.light(129, rot=(-40, 1, 1, 0))]
        compm = b.compositing(blend=comp) if comp else 0
        textured_cube_scene(b, mat, comp=compm, colors=colors, lights=lights, mesh_kw=dict(rot=(25, 1, 1, 0), alpha=180))
        b.save(os.path.join(out, name + '.m3g'))
    b = Builder()
    mat = 0
    textured_cube_scene(b, mat, default_color=0xC0FF8040, comp=b.compositing(blend=64), mesh_kw=dict(rot=(25, 1, 1, 0), alpha=128))
    b.save(os.path.join(out, 'default_color_alpha.m3g'))


def scene_background(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    bg = b.background(0xFF204060)
    textured_cube_scene(b, mat, background=bg, mesh_kw=dict(rot=(25, 1, 1, 0)))
    b.save(os.path.join(out, 'background_color.m3g'))


def scene_invisible_alpha_scope(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    comp = b.compositing(blend=64)
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [], default=0xFFFF8040)
    ib = b.strips(lens, idx)
    ap = b.appearance(comp=comp, mat=0)
    hidden = b.mesh(vb, [(ib, ap)], t=(-2, 0, 0), render=False, s=(0.5, 0.5, 0.5))
    half = b.mesh(vb, [(ib, ap)], t=(0, 0, 0), alpha=128, s=(0.7, 0.7, 0.7), rot=(30, 1, 1, 0))
    scoped = b.mesh(vb, [(ib, ap)], t=(2, 0, 0), scope=2, s=(0.5, 0.5, 0.5))
    grp = b.group([half], alpha=128)
    cam = b.camera_persp(t=(0, 0, 7))
    b.world([hidden, grp, scoped, cam], camera=cam)
    b.save(os.path.join(out, 'visibility_alpha_scope.m3g'))


def scene_parallel_camera(out):
    b = Builder()
    mat = b.material(ambient=0xFFFFFF, diffuse=0xFFFFFFFF)
    img = b.image(99, 8, 8, checker(8, [255, 255, 255], [40, 80, 200]))
    tex = b.texture(img)
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    uref = b.vertex_array(2, 2, uv)
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0))])
    ib = b.strips(lens, idx)
    ap = b.appearance(mat=mat, textures=[tex])
    m = b.mesh(vb, [(ib, ap)], rot=(25, 1, 1, 0))
    cam = b.camera_parallel(height=4.0, t=(0.5, 0.5, 6), rot=(10, 0, 1, 0))
    b.world([m, cam], camera=cam)
    b.save(os.path.join(out, 'parallel_camera.m3g'))


def scene_animations(out):
    """Every interpolation kind and several animatable properties in one world."""
    b = Builder()
    mat = b.material(ambient=0x404040, diffuse=0xFF8080FF, specular=0xFFFFFF, shininess=20.0)
    mat_trk_color = None
    ctrl = b.controller()
    ctrl_half = b.controller(speed=0.5, weight=1.0, ref_seq=100.0, ref_world=0)
    ctrl_win = b.controller(active=(300, 1200))

    # translation: LINEAR / STEP / SPLINE / CONSTANT repeat / LOOP; orientation: SLERP / SQUAD; scale
    tr_lin = b.track(b.keyframes(176, 193, 800, 3, [(0, (-2, 0, 0)), (400, (-2, 1, 0)), (800, (-2, 0, 0))]), ctrl, 275)
    tr_step = b.track(b.keyframes(180, 193, 800, 3, [(0, (0, 0, 0)), (200, (0, 1, 0)), (500, (0, 0.5, 0)), (800, (0, 0, 0))]), ctrl, 275)
    tr_spl = b.track(b.keyframes(178, 193, 900, 3, [(0, (2, 0, 0)), (300, (2, 1, 0)), (600, (2, -1, 0)), (900, (2, 0, 0))]), ctrl, 275)
    tr_const = b.track(b.keyframes(176, 192, 600, 3, [(0, (-2, -2, 0)), (600, (-2, -1, 0))]), ctrl_half, 275)
    rot_slerp = b.track(b.keyframes(177, 193, 900, 4, [(0, (0, 0, 0, 1)), (300, (0, 0.7071068, 0, 0.7071068)), (600, (0.5, 0.5, 0.5, 0.5)), (900, (0, 0, 0, 1))]), ctrl, 268)
    rot_squad = b.track(b.keyframes(179, 193, 900, 4, [(0, (0, 0, 0, 1)), (300, (0.3, 0.6, 0.1, 0.7)), (600, (0.7, 0.1, 0.5, 0.5)), (900, (0, 0, 0, 1))]), ctrl, 268)
    scale3 = b.track(b.keyframes(176, 193, 700, 3, [(0, (1, 1, 1)), (350, (0.5, 1.5, 1)), (700, (1, 1, 1))], encoding=1, bias=(0.4, 0.4, 0.4), scale=(1.2, 1.2, 1.2)), ctrl, 270)
    scale1 = b.track(b.keyframes(176, 193, 700, 1, [(0, (1.0,)), (350, (0.6,)), (700, (1.0,))], encoding=2, bias=(0.5,), scale=(0.6,)), ctrl_win, 270)
    vis = b.track(b.keyframes(180, 193, 1000, 1, [(0, (1.0,)), (500, (0.0,))]), ctrl, 276)

    def cube_mesh(tracks, **kw):
        pos, nrm, uv, idx, lens = cube(b)
        pref = b.vertex_array(3, 2, pos)
        nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
        uref = b.vertex_array(2, 2, uv)
        vb = b.vertex_buffer(pref, 0.0004, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0))])
        ib = b.strips(lens, idx)
        img = b.image(99, 8, 8, checker(8, [255, 255, 255], [60, 60, 200]))
        tex = b.texture(img)
        ap = b.appearance(mat=mat, textures=[tex])
        return b.mesh(vb, [(ib, ap)], tracks=tracks, **kw)

    meshes = [
        cube_mesh([tr_lin, rot_slerp, scale3]),
        cube_mesh([tr_step, rot_squad], rot=(0, 0, 0, 1)),
        cube_mesh([tr_spl, scale1]),
        cube_mesh([tr_const, vis]),
    ]

    # light animation: colour, intensity
    lk = b.keyframes(176, 193, 1000, 3, [(0, (1, 1, 1)), (500, (1, 0.2, 0.2)), (1000, (1, 1, 1))])
    li = b.keyframes(176, 193, 1000, 1, [(0, (0.3,)), (500, (1.5,)), (1000, (0.3,))])
    light = b.light(130, color=0xFFFFFF, intensity=1.0, t=(0, 2, 4), tracks=[b.track(lk, ctrl, 258), b.track(li, ctrl, 265)])

    # material animation (diffuse colour / shininess), fog density, background colour, camera fov
    mat2 = b.material(ambient=0x404040, diffuse=0xFFFFFFFF, specular=0xFFFFFFFF, shininess=10.0,
                      tracks=[b.track(b.keyframes(176, 193, 1000, 3, [(0, (1, 0, 0)), (500, (0, 1, 0)), (1000, (1, 0, 0))]), ctrl, 261),
                              b.track(b.keyframes(176, 193, 1000, 1, [(0, (4.0,)), (500, (60.0,)), (1000, (4.0,))]), ctrl, 271)])
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    vb = b.vertex_buffer(pref, 0.0004, (0, 0, 0), nref, 0, [])
    ib = b.strips(lens, idx)
    ap = b.appearance(mat=mat2, fog=b.fog(0x203040, 80, 0.05, 0.0) if False else 0)
    meshes.append(b.mesh(vb, [(ib, ap)], t=(0, -2.2, 0), rot=(30, 0, 1, 0)))

    bg = b.background(0xFF000000, tracks=[b.track(b.keyframes(176, 193, 1000, 3, [(0, (0, 0, 0.3)), (500, (0.3, 0.1, 0)), (1000, (0, 0, 0.3))]), ctrl, 258)])
    cam = b.camera_perspective_animated = None
    camera = b.camera_persp(fovy=45.0, t=(0, 0, 9), tracks=[b.track(b.keyframes(176, 193, 1000, 1, [(0, (30.0,)), (500, (60.0,)), (1000, (30.0,))]), ctrl, 264)])
    b.world(meshes + [light, camera], camera=camera, background=bg)
    b.save(os.path.join(out, 'animations.m3g'))


def scene_morph(out):
    """Morphing mesh with two targets, normals and colours."""
    b = Builder()
    mat = b.material(ambient=0x404040, diffuse=0xFFFFFFFF, specular=0xFFFFFF, shininess=16.0, track=True)
    pos, nrm, uv, idx, lens = cube(b)
    pos2 = [int(v * 0.5) if i % 3 == 0 else v for i, v in enumerate(pos)]
    pos3 = [v + (300 if i % 3 == 1 else 0) for i, v in enumerate(pos)]
    cols = [(i * 29) & 255 if (i % 4) != 3 else 255 for i in range(24 * 4)]
    cols2 = [255 - c if (i % 4) != 3 else 255 for i, c in enumerate(cols)]
    base_p = b.vertex_array(3, 2, pos)
    t1_p = b.vertex_array(3, 2, pos2)
    t2_p = b.vertex_array(3, 2, pos3)
    base_n = b.vertex_array(3, 2, [v * 257 for v in nrm])
    t1_n = b.vertex_array(3, 2, [v * 257 for v in nrm])
    t2_n = b.vertex_array(3, 2, [-v * 257 for v in nrm])
    base_c = b.color_array(4, cols)
    t1_c = b.color_array(4, cols2)
    t2_c = b.color_array(4, cols)
    uref = b.vertex_array(2, 2, uv)
    img = b.image(99, 8, 8, checker(8, [255, 255, 255], [60, 60, 200]))
    tex = b.texture(img)
    vb = b.vertex_buffer(base_p, 0.001, (0, 0, 0), base_n, base_c, [(uref, 0.001, (0, 0, 0))])
    vt1 = b.vertex_buffer(t1_p, 0.001, (0, 0, 0), t1_n, t1_c, [(uref, 0.001, (0, 0, 0))])
    vt2 = b.vertex_buffer(t2_p, 0.001, (0, 0, 0), t2_n, t2_c, [(uref, 0.001, (0, 0, 0))])
    ib = b.strips(lens, idx)
    ap = b.appearance(mat=mat, textures=[tex])
    ctrl = b.controller()
    wk = b.keyframes(176, 193, 800, 2, [(0, (0.0, 0.0)), (400, (1.0, 0.5)), (800, (0.0, 0.0))])
    light = b.light(129, rot=(-40, 1, 1, 0))
    m = b.morphing_mesh(vb, [vt1, vt2], [(ib, ap)], tracks=[b.track(wk, ctrl, 266)], rot=(25, 1, 1, 0))
    cam = b.camera_persp(t=(0, 0, 6))
    b.world([light, m, cam], camera=cam)
    b.save(os.path.join(out, 'morph_normals_colors.m3g'))


def scene_skinned(out):
    """A skinned mesh bent by two bones; the second bone is animated."""
    b = Builder()
    mat = b.material(ambient=0x404040, diffuse=0xFFFFFFFF, specular=0xFFFFFF, shininess=16.0)
    pos, nrm, uv, idx, lens = cube(b)
    pref = b.vertex_array(3, 2, pos)
    nref = b.vertex_array(3, 2, [v * 257 for v in nrm])
    uref = b.vertex_array(2, 2, uv)
    img = b.image(99, 8, 8, checker(8, [255, 255, 255], [60, 200, 60]))
    tex = b.texture(img)
    vb = b.vertex_buffer(pref, 0.001, (0, 0, 0), nref, 0, [(uref, 0.001, (0, 0, 0))])
    ib = b.strips(lens, idx)
    ap = b.appearance(mat=mat, textures=[tex])
    ctrl = b.controller()
    rot = b.keyframes(177, 193, 1000, 4, [(0, (0, 0, 0, 1)), (500, (0.3826834, 0, 0, 0.9238795)), (1000, (0, 0, 0, 1))])
    bone1 = b.group([], t=(0, 0, 0))
    bone2 = b.group([], t=(0, 1, 0), tracks=[b.track(rot, ctrl, 268)])
    skeleton = b.group([bone1, bone2])
    light = b.light(129, rot=(-40, 1, 1, 0))
    skin = b.skinned_mesh(vb, [(ib, ap)], skeleton, [(bone1, 0, 12, 1), (bone2, 0, 24, 1), (bone1, 12, 12, 1)], rot=(25, 0, 1, 0))
    cam = b.camera_persp(t=(0, 0, 6))
    b.world([light, skin, cam], camera=cam)
    b.save(os.path.join(out, 'skinned.m3g'))


SCENES = [scene_lights, scene_lights_local_viewer, scene_flat_twosided, scene_smooth_twosided, scene_cull_front,
          scene_fog, scene_tex_formats, scene_tex_wrap_filter_transform, scene_two_units, scene_blend_modes,
          scene_alpha_test, scene_depth_flags, scene_vertex_colors, scene_background,
          scene_invisible_alpha_scope, scene_parallel_camera, scene_animations, scene_morph, scene_skinned]

if __name__ == '__main__':
    out = sys.argv[1] if len(sys.argv) > 1 else '.'
    os.makedirs(out, exist_ok=True)
    for fn in SCENES:
        fn(out)
    print(len(os.listdir(out)), 'files in', out)
