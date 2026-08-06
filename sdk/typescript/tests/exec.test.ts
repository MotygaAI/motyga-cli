import * as child_process from "node:child_process";
import { EventEmitter } from "node:events";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";

import { describe, expect, it } from "@jest/globals";

jest.mock("node:child_process", () => {
  const actual = jest.requireActual<typeof import("node:child_process")>("node:child_process");
  return { ...actual, spawn: jest.fn() };
});

const _actualChildProcess =
  jest.requireActual<typeof import("node:child_process")>("node:child_process");
const spawnMock = child_process.spawn as jest.MockedFunction<typeof _actualChildProcess.spawn>;

class FakeChildProcess extends EventEmitter {
  stdin = new PassThrough();
  stdout = new PassThrough();
  stderr = new PassThrough();
  killed = false;

  kill(): boolean {
    this.killed = true;
    return true;
  }
}

function createEarlyExitChild(exitCode = 2): FakeChildProcess {
  const child = new FakeChildProcess();
  setImmediate(() => {
    child.stderr.write("boom");
    child.emit("exit", exitCode, null);
    setImmediate(() => {
      child.stdout.end();
      child.stderr.end();
    });
  });
  return child;
}

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe("MotygaExec", () => {
  it("rejects when exit happens before stdout closes", async () => {
    const { MotygaExec } = await import("../src/exec");
    const child = createEarlyExitChild();
    spawnMock.mockReturnValue(child as unknown as child_process.ChildProcess);

    const exec = new MotygaExec("motyga");
    const runPromise = (async () => {
      for await (const _ of exec.run({ input: "hi" })) {
        // no-op
      }
    })().then(
      () => ({ status: "resolved" as const }),
      (error) => ({ status: "rejected" as const, error }),
    );

    const result = await Promise.race([
      runPromise,
      delay(500).then(() => ({ status: "timeout" as const })),
    ]);

    expect(result.status).toBe("rejected");
    if (result.status === "rejected") {
      expect(result.error).toBeInstanceOf(Error);
      expect(result.error.message).toMatch(/Motyga Exec exited/);
    }
  });

  it("places resume args before image args", async () => {
    const { MotygaExec } = await import("../src/exec");
    spawnMock.mockClear();
    const child = new FakeChildProcess();
    spawnMock.mockReturnValue(child as unknown as child_process.ChildProcess);

    setImmediate(() => {
      child.stdout.end();
      child.stderr.end();
      child.emit("exit", 0, null);
    });

    const exec = new MotygaExec("motyga");
    for await (const _ of exec.run({ input: "hi", images: ["img.png"], threadId: "thread-id" })) {
      // no-op
    }

    const commandArgs = spawnMock.mock.calls[0]?.[1] as string[] | undefined;
    expect(commandArgs).toBeDefined();
    const resumeIndex = commandArgs!.indexOf("resume");
    const imageIndex = commandArgs!.indexOf("--image");
    expect(resumeIndex).toBeGreaterThan(-1);
    expect(imageIndex).toBeGreaterThan(-1);
    expect(resumeIndex).toBeLessThan(imageIndex);
  });

  it("allows overriding the env passed to the Motyga CLI", async () => {
    const { MotygaExec } = await import("../src/exec");
    spawnMock.mockClear();
    const child = new FakeChildProcess();
    spawnMock.mockReturnValue(child as unknown as child_process.ChildProcess);

    setImmediate(() => {
      child.stdout.end();
      child.stderr.end();
      child.emit("exit", 0, null);
    });

    process.env.MOTYGA_ENV_SHOULD_NOT_LEAK = "leak";

    try {
      const exec = new MotygaExec("motyga", {
        MOTYGA_HOME: "/tmp/motyga-home",
        CUSTOM_ENV: "custom",
      });

      for await (const _ of exec.run({
        input: "custom env",
        apiKey: "test",
        baseUrl: "https://example.test",
      })) {
        // no-op
      }

      const commandArgs = spawnMock.mock.calls[0]?.[1] as string[] | undefined;
      expect(commandArgs).toBeDefined();
      const spawnOptions = spawnMock.mock.calls[0]?.[2] as child_process.SpawnOptions | undefined;
      const spawnEnv = spawnOptions?.env as Record<string, string> | undefined;
      expect(spawnEnv).toBeDefined();
      if (!spawnEnv || !commandArgs) {
        throw new Error("Spawn args missing");
      }

      expect(spawnEnv.MOTYGA_HOME).toBe("/tmp/motyga-home");
      expect(spawnEnv.CUSTOM_ENV).toBe("custom");
      expect(spawnEnv.MOTYGA_ENV_SHOULD_NOT_LEAK).toBeUndefined();
      expect(spawnEnv.MOTYGA_API_KEY).toBe("test");
      expect(spawnEnv.MOTYGA_INTERNAL_ORIGINATOR_OVERRIDE).toBeDefined();
      expect(commandArgs).toContain("--config");
      expect(commandArgs).toContain(`openai_base_url=${JSON.stringify("https://example.test")}`);
    } finally {
      delete process.env.MOTYGA_ENV_SHOULD_NOT_LEAK;
    }
  });

  it("resolves the package-layout binary and PATH directory", async () => {
    const { resolveNativePackage } = await import("../src/exec");
    const vendorRoot = mkdtempSync(path.join(tmpdir(), "motyga-sdk-vendor-"));
    const packageRoot = path.join(vendorRoot, "x86_64-unknown-linux-gnu");
    const binDir = path.join(packageRoot, "bin");
    const pathDir = path.join(packageRoot, "motyga-path");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pathDir, { recursive: true });
    writeFileSync(path.join(packageRoot, "motyga-package.json"), "{}");
    writeFileSync(path.join(binDir, "motyga"), "");

    expect(resolveNativePackage(vendorRoot, "x86_64-unknown-linux-gnu", "motyga")).toEqual({
      executablePath: path.join(binDir, "motyga"),
      pathDirs: [pathDir],
    });
  });

  it("falls back to the legacy binary layout", async () => {
    const { resolveNativePackage } = await import("../src/exec");
    const vendorRoot = mkdtempSync(path.join(tmpdir(), "motyga-sdk-vendor-"));
    const packageRoot = path.join(vendorRoot, "x86_64-unknown-linux-gnu");
    const binDir = path.join(packageRoot, "motyga");
    const pathDir = path.join(packageRoot, "path");
    mkdirSync(binDir, { recursive: true });
    mkdirSync(pathDir, { recursive: true });
    writeFileSync(path.join(binDir, "motyga"), "");

    expect(resolveNativePackage(vendorRoot, "x86_64-unknown-linux-gnu", "motyga")).toEqual({
      executablePath: path.join(binDir, "motyga"),
      pathDirs: [pathDir],
    });
  });

  it("prepends package PATH entries without duplicating them", async () => {
    const { prependPathDirs } = await import("../src/exec");
    const pathDir = path.join(tmpdir(), "motyga-path");
    const env = { PATH: `/usr/bin${path.delimiter}${pathDir}` };

    prependPathDirs(env, [pathDir]);

    expect(env).toEqual({ PATH: `${pathDir}${path.delimiter}/usr/bin` });
  });

  it("preserves the Windows Path key when prepending package PATH entries", async () => {
    const { prependPathDirs } = await import("../src/exec");
    const pathDir = path.join(tmpdir(), "motyga-path");
    const env = { PATH: "/usr/bin", Path: `C\\Windows${path.delimiter}${pathDir}` };

    prependPathDirs(env, [pathDir], "win32");

    expect(env).toEqual({ Path: `${pathDir}${path.delimiter}C\\Windows` });
  });
});
