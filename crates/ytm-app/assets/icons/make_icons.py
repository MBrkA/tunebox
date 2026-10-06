#!/usr/bin/env python3
"""Renders the Tunebox app icon (pure Python, no dependencies).
Usage: python3 make_icons.py   -> writes 32/128/256/512/1024 PNGs and icon.png next to this file."""
import math, os, struct, zlib, shutil

HERE = os.path.dirname(os.path.abspath(__file__))

def write_png(path, size, px):
    raw = b"".join(b"\x00" + bytes(px[y * size * 4:(y + 1) * size * 4]) for y in range(size))
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
                           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))

def lerp(a, b, t): return a + (b - a) * t
def mix(c1, c2, t): return tuple(lerp(a, b, t) for a, b in zip(c1, c2))
def smooth(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0))); return t * t * (3 - 2 * t)

def sd_round_box(x, y, hw, hh, r):
    qx, qy = abs(x) - hw + r, abs(y) - hh + r
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r

def sd_triangle(px, py, a, b, c):
    # signed distance to a triangle (iq's formulation)
    def seg(p, q):
        ex, ey = q[0] - p[0], q[1] - p[1]; wx, wy = px - p[0], py - p[1]
        t = max(0, min(1, (wx * ex + wy * ey) / (ex * ex + ey * ey)))
        return (wx - ex * t) ** 2 + (wy - ey * t) ** 2
    d = min(seg(a, b), seg(b, c), seg(c, a))
    def s(p, q): return (q[0] - p[0]) * (py - p[1]) - (q[1] - p[1]) * (px - p[0])
    inside = (s(a, b) >= 0) == (s(b, c) >= 0) == (s(c, a) >= 0)
    return -math.sqrt(d) if inside else math.sqrt(d)

def over(dst, src):
    """premultiplied-free 'over' for (r,g,b,a) tuples, a in 0..1"""
    sa, da = src[3], dst[3]
    oa = sa + da * (1 - sa)
    if oa <= 0: return (0, 0, 0, 0)
    return tuple((src[i] * sa + dst[i] * da * (1 - sa)) / oa for i in range(3)) + (oa,)

def shade(x, y, aa):
    """x,y in [-0.5,0.5]; aa = pixel size in the same units."""
    px = (0, 0, 0, 0)
    # squircle plate with vertical gradient and a faint top highlight
    d = sd_round_box(x, y, 0.5, 0.5, 0.225)
    cov = 1 - smooth(-aa, aa, d)
    if cov <= 0: return px
    plate = mix((0x26, 0x26, 0x2B), (0x0B, 0x0B, 0x0D), (y + 0.5))
    plate += (cov,)
    px = over(px, plate)
    # red disc: radial gradient + soft glow
    r = math.hypot(x, y)
    glow = max(0.0, 1 - r / 0.50) ** 2 * 0.35
    px = over(px, (0xFF, 0x00, 0x33, glow * cov))
    dd = r - 0.335
    dcov = 1 - smooth(-aa, aa, dd)
    if dcov > 0:
        t = max(0.0, min(1.0, ((x * -0.35) + (y * -0.9)) * 0.9 + 0.5))   # light from the top-left
        col = mix((0xC8, 0x00, 0x2A), (0xFF, 0x4D, 0x6D), t)
        px = over(px, col + (dcov * cov,))
        # inner rim light
        rim = smooth(0.30, 0.335, r) * (1 - smooth(0.335, 0.34, r))
        px = over(px, (255, 255, 255, 0.10 * rim * cov))
    # two faint "sound" rings
    for rad, alpha in ((0.405, 0.22), (0.455, 0.10)):
        ring = 1 - smooth(0.004 - aa, 0.004 + aa, abs(r - rad))
        px = over(px, (0xFF, 0x33, 0x55, alpha * ring * cov))
    # rounded white play triangle with a soft drop shadow
    a, b, c = (-0.085, -0.155), (-0.085, 0.155), (0.165, 0.0)
    rr = 0.028
    ts = sd_triangle(x - 0.0, y - 0.012, a, b, c) - rr
    sh = (1 - smooth(-0.02, 0.03, ts)) * 0.28
    px = over(px, (0x60, 0x00, 0x14, sh * cov * (1 if dcov > 0 else 0)))
    tcov = 1 - smooth(-aa, aa, sd_triangle(x, y, a, b, c) - rr)
    if tcov > 0:
        px = over(px, mix((255, 255, 255), (0xF0, 0xF0, 0xF2), y + 0.5) + (tcov * cov,))
    return px

def render(size, ss):
    out = bytearray(size * size * 4)
    aa = 1.0 / size
    for j in range(size):
        for i in range(size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(ss):
                for sx in range(ss):
                    x = (i + (sx + 0.5) / ss) / size - 0.5
                    y = (j + (sy + 0.5) / ss) / size - 0.5
                    r, g, b, a = shade(x, y, aa)
                    acc[0] += r * a; acc[1] += g * a; acc[2] += b * a; acc[3] += a
            if acc[3] > 0:
                k = (j * size + i) * 4
                out[k] = int(acc[0] / acc[3]); out[k + 1] = int(acc[1] / acc[3]); out[k + 2] = int(acc[2] / acc[3])
                out[k + 3] = int(255 * acc[3] / (ss * ss))
    return out

if __name__ == "__main__":
    for s, ss in ((32, 6), (128, 4), (256, 3), (512, 3), (1024, 2)):
        write_png(os.path.join(HERE, f"{s}x{s}.png"), s, render(s, ss)); print("wrote", s)
    shutil.copy(os.path.join(HERE, "512x512.png"), os.path.join(HERE, "icon.png"))
