#!/usr/bin/env python3
"""Rasterize packaging/hark.svg's simple ring without build-time dependencies."""
import math
from pathlib import Path
import struct
import sys
import zlib


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def icon(size):
    scale = size / 256
    rows = bytearray()
    for y in range(size):
        rows.append(0)  # PNG scanline filter: none
        for x in range(size):
            dx, dy = abs(x + 0.5 - size / 2), abs(y + 0.5 - size / 2)
            # Signed distance to the rounded rectangle and to the accent ring.
            corner = 56 * scale
            qx, qy = dx - (size / 2 - corner), dy - (size / 2 - corner)
            edge = math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - corner
            alpha = min(1, max(0, 0.5 - edge))
            ring = min(1, max(0, 0.5 + 20 * scale - abs(math.hypot(dx, dy) - 84 * scale)))
            rgb = [round(base + ring * (accent - base)) for base, accent in zip((43, 39, 65), (145, 132, 217))]
            rows.extend((*rgb, round(255 * alpha)))
    header = struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')


if __name__ == '__main__':
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    for points in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            suffix = '@2x' if scale == 2 else ''
            (output / f'icon_{points}x{points}{suffix}.png').write_bytes(icon(points * scale))
