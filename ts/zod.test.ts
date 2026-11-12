// The generated Zod schemas accept and reject example values, and every
// key default parses with its own schema.
import { describe, expect, test } from "bun:test";

import { ContactSchema, LimitSchema, type McpServer, McpServerSchema, keys } from "../examples/basic/gen/zod/zod.ts";
import * as inline from "../examples/basic/gen/zod-inline/zod.ts";
import type * as Bare from "../examples/basic/gen/ts/types.ts";
import { keyDefaults } from "../examples/basic/gen/ts/types.ts";

describe("tagged union", () => {
  test("accepts each variant", () => {
    const stdio: McpServer = McpServerSchema.parse({ type: "stdio", command: "mcp", args: ["-v"] });
    expect(stdio).toEqual({ type: "stdio", command: "mcp", args: ["-v"] });
    // The field default fills in.
    expect(McpServerSchema.parse({ type: "http", url: "https://x.test" })).toEqual({
      type: "http",
      url: "https://x.test",
      timeout_ms: 30000,
    });
  });

  test("rejects bad values with custom messages", () => {
    expect(McpServerSchema.safeParse({ type: "ftp" }).success).toBe(false);
    expect(McpServerSchema.safeParse({ type: "stdio", command: "" }).success).toBe(false);
    const r = McpServerSchema.safeParse({ type: "http", url: "http://x.test" });
    expect(r.success).toBe(false);
    expect(r.error!.issues.map((i) => i.message)).toContain("MCP servers must use HTTPS");
  });
});

test("untagged union", () => {
  expect(LimitSchema.parse(5)).toBe(5);
  expect(LimitSchema.parse("max")).toBe("max");
  expect(LimitSchema.safeParse(-1).success).toBe(false);
});

test("struct rules", () => {
  const ok = {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Ada",
    emails: ["ada@example.com"],
    score: 0.5,
    avatar: "AAEC",
    created: "2020-01-01T00:00:00Z",
  };
  expect(ContactSchema.parse(ok)).toEqual(ok);
  expect(ContactSchema.safeParse({ ...ok, score: 2 }).success).toBe(false);
  expect(ContactSchema.safeParse({ ...ok, emails: ["a@x.test", "a@x.test"] }).success).toBe(false);
  expect(ContactSchema.safeParse({ ...ok, emails: ["nope"] }).success).toBe(false);
  expect(ContactSchema.safeParse({ ...ok, created: "2999-01-01T00:00:00Z" }).success).toBe(false);
  expect(inline.ContactSchema.parse(ok)).toEqual(ok);
});

test("key defaults parse with their schemas", () => {
  for (const [name, k] of Object.entries(keys)) {
    if (k.default !== undefined) {
      expect(k.schema.safeParse(k.default).success, name).toBe(true);
    }
  }
  expect(keys["session.timeout"].schema.safeParse("0.5s").success).toBe(false);
  expect(keys["claude.api_key"].secret).toBe(true);
  const d: Partial<Bare.Keys> = keyDefaults;
  expect(d["session.backend"]).toBe("claude");
});
