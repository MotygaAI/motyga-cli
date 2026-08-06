import path from "node:path";

export function motygaPathOverride() {
  return (
    process.env.MOTYGA_EXECUTABLE ??
    path.join(process.cwd(), "..", "..", "motyga-rs", "target", "debug", "motyga")
  );
}
