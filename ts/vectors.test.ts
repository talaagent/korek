// The shared validation vectors against the generated Zod schemas.
import { expect, test } from "bun:test";

import vectors from "../crates/korek-testkit/vectors/rules.json";
import * as gen from "../crates/korek-testkit/gen/zod/zod.ts";

const structName = (name: string) =>
  "V" + name.split("_").map((w) => w[0]!.toUpperCase() + w.slice(1)).join("");

for (const c of vectors.cases) {
  test(c.name, () => {
    const schema = (gen as unknown as Record<string, { safeParse(v: unknown): { success: boolean } }>)[
      `${structName(c.name)}Schema`
    ];
    expect(schema).toBeDefined();
    for (const v of c.valid) {
      expect(schema!.safeParse({ v }).success, `${c.name}: ${JSON.stringify(v)} should pass`).toBe(true);
    }
    for (const v of c.invalid) {
      expect(schema!.safeParse({ v }).success, `${c.name}: ${JSON.stringify(v)} should fail`).toBe(false);
    }
  });
}
