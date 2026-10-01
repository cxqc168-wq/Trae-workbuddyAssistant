import { createServer } from 'node:net';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Windows may allow IPv4 and IPv6 to bind the same port independently.
function portAvailable(port, host) {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once('error', (error) => {
      if (error.code === 'EADDRINUSE' || error.code === 'EACCES') {
        resolvePort(false);
      } else if (host === '::1' && ['EAFNOSUPPORT', 'EADDRNOTAVAIL'].includes(error.code)) {
        resolvePort(true); // IPv6 is disabled; the IPv4 probe still applies.
      } else {
        reject(error);
      }
    });
    server.listen(port, host, () => server.close(() => resolvePort(true)));
  });
}

export async function findAvailablePort(startPort = 5188) {
  for (let port = startPort; port <= 65535; port++) {
    if (await portAvailable(port, '127.0.0.1') && await portAvailable(port, '::1')) return port;
  }
  throw new Error(`No available development port from ${startPort} to 65535`);
}

export function devConfig(port) {
  return {
    build: {
      devUrl: `http://127.0.0.1:${port}`,
      beforeDevCommand: `node scripts/prepare-dev.mjs && npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    },
  };
}

async function main() {
  const port = await findAvailablePort();
  console.log(`[START] Development server: http://127.0.0.1:${port}`);
  const { run } = await import('@tauri-apps/cli');
  // Pass JSON directly to the CLI, avoiding Windows command-line quote escaping.
  await run(['dev', '--config', JSON.stringify(devConfig(port))], 'start.bat');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`[ERROR] ${error.message}`);
    process.exitCode = 1;
  });
}
