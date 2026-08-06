import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, beforeEach } from "@jest/globals";

const originalMotygaHome = process.env.MOTYGA_HOME;
let currentMotygaHome: string | undefined;

beforeEach(async () => {
  currentMotygaHome = await fs.mkdtemp(path.join(os.tmpdir(), "motyga-sdk-test-"));
  process.env.MOTYGA_HOME = currentMotygaHome;
});

afterEach(async () => {
  const motygaHomeToDelete = currentMotygaHome;
  currentMotygaHome = undefined;

  if (originalMotygaHome === undefined) {
    delete process.env.MOTYGA_HOME;
  } else {
    process.env.MOTYGA_HOME = originalMotygaHome;
  }

  if (motygaHomeToDelete) {
    await fs.rm(motygaHomeToDelete, { recursive: true, force: true });
  }
});
