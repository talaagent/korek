// The shared validation vectors against the generated Protobuf, checked by
// protovalidate-es. The descriptors come from `buf build` of the golden
// crates/korek-testkit/gen/proto (buf is in the dev shell), so no generated
// TS is needed: messages are looked up by name in a registry.
import { expect, test } from "bun:test";
import { createFileRegistry, fromBinary, fromJson, type DescMessage } from "@bufbuild/protobuf";
import { FileDescriptorSetSchema } from "@bufbuild/protobuf/wkt";
import { createValidator } from "@bufbuild/protovalidate";

import vectors from "../crates/korek-testkit/vectors/rules.json";

const build = Bun.spawnSync(["buf", "build", "-o", "-#format=binpb"], {
  cwd: `${import.meta.dir}/../crates/korek-testkit/gen/proto`,
});
if (!build.success) throw new Error(`buf build failed: ${build.stderr.toString()}`);
const registry = createFileRegistry(fromBinary(FileDescriptorSetSchema, build.stdout));
const validator = createValidator({ registry });

const structName = (name: string) =>
  "V" + name.split("_").map((w) => w[0]!.toUpperCase() + w.slice(1)).join("");

/** Whether `{v: value}` parses as the message and passes protovalidate. */
function accepts(desc: DescMessage, v: unknown): boolean {
  let msg;
  try {
    msg = fromJson(desc, { v } as never);
  } catch {
    return false;
  }
  const r = validator.validate(desc, msg);
  if (r.kind === "error") throw r.error;
  return r.kind === "valid";
}

for (const c of vectors.cases) {
  test(c.name, () => {
    const desc = registry.getMessage(`vectors.v1.${structName(c.name)}`);
    expect(desc).toBeDefined();
    for (const v of c.valid) {
      expect(accepts(desc!, v), `${c.name}: ${JSON.stringify(v)} should pass`).toBe(true);
    }
    for (const v of c.invalid) {
      expect(accepts(desc!, v), `${c.name}: ${JSON.stringify(v)} should fail`).toBe(false);
    }
  });
}
