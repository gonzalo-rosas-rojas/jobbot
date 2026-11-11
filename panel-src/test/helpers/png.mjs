// test/helpers/png.mjs — decodes the PNG that Page.captureScreenshot returns (8 bits, RGB or RGBA, not interlaced) into pixels, so a
// test can measure colours of what Chrome really painted: const { w, h, at } = decodePng(buffer); at(x, y) -> [r, g, b].
import { inflateSync } from "node:zlib";

export function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("not a PNG");
  let w = 0, h = 0, depth = 0, type = 0, interlace = 0;
  const idat = [];
  for (let p = 8; p < buf.length; ) {
    const len = buf.readUInt32BE(p), name = buf.toString("latin1", p + 4, p + 8), body = buf.subarray(p + 8, p + 8 + len);
    if (name === "IHDR") { w = body.readUInt32BE(0); h = body.readUInt32BE(4); depth = body[8]; type = body[9]; interlace = body[12]; }
    else if (name === "IDAT") idat.push(body);
    else if (name === "IEND") break;
    p += 12 + len;
  }
  if (depth !== 8 || (type !== 2 && type !== 6) || interlace) throw new Error(`unsupported PNG (depth ${depth}, type ${type}, interlace ${interlace})`);
  const bpp = type === 6 ? 4 : 3, stride = w * bpp;
  const raw = inflateSync(Buffer.concat(idat));
  const px = new Uint8Array(h * stride);
  for (let y = 0; y < h; y++) {
    const filter = raw[y * (stride + 1)], line = y * (stride + 1) + 1, out = y * stride;
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? px[out + i - bpp] : 0, b = y ? px[out - stride + i] : 0, c = y && i >= bpp ? px[out - stride + i - bpp] : 0;
      let v = raw[line + i];
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) { const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      px[out + i] = v & 255;
    }
  }
  return { w, h, at: (x, y) => { const o = y * stride + x * bpp; return [px[o], px[o + 1], px[o + 2]]; } };
}
