#!/usr/bin/env node
// Upload a Thruster binary through the chunked endpoint.
// Usage:
//   SEBRUS_SID=<your-sid-cookie> node scripts/upload-thruster-chunked.js \
//     <platform> <local-file> [base-url]
//
// Get SEBRUS_SID by logging into the Sebrus Store in your browser and copying
// the value of the `sid` cookie.

const fs = require('fs');
const path = require('path');

const CHUNK_SIZE = 5 * 1024 * 1024; // must stay under the 20 MB route limit

async function uploadChunk(url, cookie, platform, filename, uploadId, index, total, chunk) {
  const fd = new FormData();
  fd.append('platform', platform);
  fd.append('filename', filename);
  fd.append('upload_id', uploadId);
  fd.append('chunk_index', String(index));
  fd.append('total_chunks', String(total));
  fd.append('file', new Blob([chunk]), filename);

  const res = await fetch(url, {
    method: 'POST',
    headers: { cookie: `sid=${cookie}` },
    body: fd,
  });

  const body = await res.json().catch(() => ({}));
  if (!res.ok || body.error) {
    throw new Error(body.error || `HTTP ${res.status}`);
  }
  return body;
}

async function main() {
  const sid = process.env.SEBRUS_SID;
  const platform = process.argv[2];
  const filePath = process.argv[3];
  const baseUrl = (process.argv[4] || 'https://apps.sebrus.dev').replace(/\/$/, '');

  if (!sid || !platform || !filePath) {
    console.error('Usage: SEBRUS_SID=<sid> node scripts/upload-thruster-chunked.js <platform> <file> [base-url]');
    process.exit(1);
  }

  if (!['windows', 'macos', 'linux'].includes(platform)) {
    console.error('platform must be windows, macos, or linux');
    process.exit(1);
  }

  const stat = fs.statSync(filePath);
  const totalSize = stat.size;
  const totalChunks = Math.ceil(totalSize / CHUNK_SIZE);
  const filename = path.basename(filePath);
  const uploadId = `u-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
  const url = `${baseUrl}/api/method/thruster.upload_chunk`;

  console.log(`Uploading ${filename} (${totalSize} bytes) to ${url} in ${totalChunks} chunk(s)`);

  const handle = fs.openSync(filePath, 'r');
  try {
    for (let index = 0; index < totalChunks; index++) {
      const start = index * CHUNK_SIZE;
      const end = Math.min(start + CHUNK_SIZE, totalSize);
      const length = end - start;
      const chunk = Buffer.alloc(length);
      fs.readSync(handle, chunk, 0, length, start);

      process.stdout.write(`Chunk ${index + 1}/${totalChunks} ... `);
      const result = await uploadChunk(url, sid, platform, filename, uploadId, index, totalChunks, chunk);
      console.log(result.completed ? 'done' : 'received');
    }
  } finally {
    fs.closeSync(handle);
  }

  console.log('Upload complete.');
}

main().catch((err) => {
  console.error('Upload failed:', err.message);
  process.exit(1);
});
