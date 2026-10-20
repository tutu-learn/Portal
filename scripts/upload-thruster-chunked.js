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

const DEFAULT_CHUNK_SIZE = 5 * 1024 * 1024; // 5 MB file payload
const MIN_CHUNK_SIZE = 512 * 1024;          // 512 KB floor

class UploadError extends Error {
  constructor(message, status) {
    super(message);
    this.status = status;
  }
}

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
    throw new UploadError(body.error || `HTTP ${res.status}`, res.status);
  }
  return body;
}

async function uploadWithChunkSize(url, sid, platform, filePath, chunkSize) {
  const stat = fs.statSync(filePath);
  const totalSize = stat.size;
  const totalChunks = Math.ceil(totalSize / chunkSize);
  const filename = path.basename(filePath);
  const uploadId = `u-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;

  console.log(
    `Uploading ${filename} (${totalSize} bytes) in ${totalChunks} chunk(s) of ${chunkSize} bytes`
  );

  const handle = fs.openSync(filePath, 'r');
  try {
    for (let index = 0; index < totalChunks; index++) {
      const start = index * chunkSize;
      const end = Math.min(start + chunkSize, totalSize);
      const length = end - start;
      const chunk = Buffer.alloc(length);
      fs.readSync(handle, chunk, 0, length, start);

      process.stdout.write(`Chunk ${index + 1}/${totalChunks} ... `);
      const result = await uploadChunk(
        url,
        sid,
        platform,
        filename,
        uploadId,
        index,
        totalChunks,
        chunk
      );
      console.log(result.completed ? 'done' : 'received');
    }
  } finally {
    fs.closeSync(handle);
  }

  console.log('Upload complete.');
}

async function tryUpload(url, sid, platform, filePath) {
  let chunkSize = DEFAULT_CHUNK_SIZE;

  while (true) {
    try {
      return await uploadWithChunkSize(url, sid, platform, filePath, chunkSize);
    } catch (err) {
      const is413 = err.status === 413 || /\b413\b/.test(err.message);
      if (is413 && chunkSize > MIN_CHUNK_SIZE) {
        const nextSize = Math.max(MIN_CHUNK_SIZE, Math.floor(chunkSize / 2));
        console.log(
          `Received 413 with chunk size ${chunkSize}; retrying with ${nextSize} bytes`
        );
        chunkSize = nextSize;
        continue;
      }
      throw err;
    }
  }
}

async function main() {
  const sid = process.env.SEBRUS_SID;
  const platform = process.argv[2];
  const filePath = process.argv[3];
  const baseUrl = (process.argv[4] || 'https://apps.sebrus.dev').replace(/\/$/, '');

  if (!sid || !platform || !filePath) {
    console.error(
      'Usage: SEBRUS_SID=<sid> node scripts/upload-thruster-chunked.js <platform> <file> [base-url]'
    );
    process.exit(1);
  }

  if (!['windows', 'macos', 'linux'].includes(platform)) {
    console.error('platform must be windows, macos, or linux');
    process.exit(1);
  }

  const url = `${baseUrl}/api/method/thruster.upload_chunk`;
  await tryUpload(url, sid, platform, filePath);
}

main().catch((err) => {
  console.error('Upload failed:', err.message);
  process.exit(1);
});
