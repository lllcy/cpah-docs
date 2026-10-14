// The official QPDF archives use ordinary (non-ZIP64) stored/deflated entries.
import { crc32, deflateRawSync, inflateRawSync } from "node:zlib";
import { posix } from "node:path";

export function readZip(bytes) {
  let end = bytes.length - 22;
  while (end >= Math.max(0, bytes.length - 65557) && bytes.readUInt32LE(end) !== 0x06054b50) end--;
  if (end < 0 || bytes.readUInt32LE(end) !== 0x06054b50) throw new Error("ZIP directory missing");
  const entries = new Map();
  let cursor = bytes.readUInt32LE(end + 16);
  for (let index = 0; index < bytes.readUInt16LE(end + 10); index++) {
    if (bytes.readUInt32LE(cursor) !== 0x02014b50) throw new Error("Invalid ZIP directory");
    const nameLength = bytes.readUInt16LE(cursor + 28);
    const name = bytes.toString("utf8", cursor + 46, cursor + 46 + nameLength);
    if (name.startsWith("/") || name.includes("\\") || name.split("/").includes("..")) throw new Error("Unsafe ZIP path");
    const size = bytes.readUInt32LE(cursor + 24);
    const compressedSize = bytes.readUInt32LE(cursor + 20);
    const local = bytes.readUInt32LE(cursor + 42);
    if (bytes.readUInt32LE(local) !== 0x04034b50 || size > 256 * 1024 * 1024) throw new Error("Invalid ZIP entry");
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    const compressed = bytes.subarray(start, start + compressedSize);
    const method = bytes.readUInt16LE(cursor + 10);
    const data = method === 0 ? compressed : method === 8 ? inflateRawSync(compressed, { maxOutputLength: size + 1 }) : null;
    if (!data || data.length !== size || crc32(data) !== bytes.readUInt32LE(cursor + 16)) throw new Error(`ZIP checksum failed: ${name}`);
    const mode = bytes.readUInt32LE(cursor + 38) >>> 16;
    entries.set(name, { data, symlink: (mode & 0xf000) === 0xa000 });
    cursor += 46 + nameLength + bytes.readUInt16LE(cursor + 30) + bytes.readUInt16LE(cursor + 32);
  }
  function resolve(name, seen = new Set()) {
    if (seen.has(name)) throw new Error("Cyclic ZIP symlink");
    seen.add(name);
    const entry = entries.get(name);
    if (!entry) throw new Error(`ZIP entry missing: ${name}`);
    if (!entry.symlink) return entry.data;
    return resolve(posix.normalize(posix.join(posix.dirname(name), entry.data.toString("utf8"))), seen);
  }
  return new Map([...entries.keys()].filter(name => !name.endsWith("/")).map(name => [name, resolve(name)]));
}

export function writeZip(files) {
  const local = [], central = [];
  let offset = 0;
  for (const [name, data] of [...files].sort(([a], [b]) => a.localeCompare(b, "en"))) {
    const filename = Buffer.from(name);
    const compressed = deflateRawSync(data, { level: 9 });
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x800, 6);
    header.writeUInt16LE(8, 8);
    header.writeUInt16LE(33, 12); // January 1, 1980; reproducible archives.
    header.writeUInt32LE(crc32(data), 14);
    header.writeUInt32LE(compressed.length, 18);
    header.writeUInt32LE(data.length, 22);
    header.writeUInt16LE(filename.length, 26);
    const directory = Buffer.alloc(46);
    directory.writeUInt32LE(0x02014b50, 0);
    directory.writeUInt16LE(0x0314, 4);
    header.copy(directory, 6, 4, 30);
    directory.writeUInt32LE(((name.startsWith("bin/") ? 0x81ed : 0x81a4) * 65536) >>> 0, 38);
    directory.writeUInt32LE(offset, 42);
    local.push(header, filename, compressed);
    central.push(directory, filename);
    offset += header.length + filename.length + compressed.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(files.size, 8);
  end.writeUInt16LE(files.size, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, directory, end]);
}
