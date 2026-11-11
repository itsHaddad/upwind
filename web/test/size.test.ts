// Source files stay small enough to read in one sitting. A file that grows
// past the limit is a sign it holds more than one idea: split it.

import { expect, test } from "bun:test";
import { join } from "node:path";
import { Glob } from "bun";

const root = join(import.meta.dir, "../..");
const LIMIT = 320;

test(`no source file is longer than ${LIMIT} lines`, async () => {
  const tooLong: string[] = [];
  for (const pattern of ["crates/*/src/**/*.rs", "crates/*/tests/**/*.rs", "web/src/**/*.ts", "scripts/**/*.ts"]) {
    for await (const path of new Glob(pattern).scan(root)) {
      const lines = (await Bun.file(join(root, path)).text()).split("\n").length;
      if (lines > LIMIT) tooLong.push(`${path} (${lines} lines)`);
    }
  }
  expect(tooLong).toEqual([]);
});
