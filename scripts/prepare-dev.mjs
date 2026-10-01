import { mkdir, copyFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// A clean checkout can run dev builds without downloading release runtimes.
const source = fileURLToPath(new URL('../src-python/', import.meta.url));
const target = fileURLToPath(new URL('../resources/python/', import.meta.url));
await mkdir(target, { recursive: true });
for (const name of await readdir(source)) {
  if (name.endsWith('.py')) await copyFile(`${source}/${name}`, `${target}/${name}`);
}
