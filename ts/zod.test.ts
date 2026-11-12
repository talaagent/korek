// The generated Zod schemas accept and reject example values, and every
// key default parses with its own schema.
import { describe, expect, test } from "bun:test";

import {
  ContactSchema,
  type McpServer,
  McpServerSchema,
  LimitSchema,
  keys,
} from "../examples/basic/gen/zod/zod.ts";
import type * as Bare from "../examples/basic/gen/ts/types.ts";
import { sessionDefaults } from "../examples/basic/gen/ts/types.ts";

describe("tagged union", () => {
  test("accepts each variant", () => {
    const stdio: McpServer = McpServerSchema.parse({ type: "stdio", command: "mcp", args: ["-v"] });
    expect(stdio).toEqual({ type: "stdio", command: "mcp", args: ["-v"] });
    expect(McpServerSchema.parse({ type: "http", url: "https://x.test" }).type).toBe("http");
  });

  test("rejects bad values", () => {
    expect(McpServerSchema.safeParse({ type: "ftp" }).success).toBe(false);
    expect(McpServerSchema.safeParse({ type: "stdio", command: "" }).success).toBe(false);
    expect(McpServerSchema.safeParse({ type: "http", url: "ftp://x" }).success).toBe(false);
    expect(McpServerSchema.safeParse({ type: "http", url: "https://x", timeout_ms: 0 }).success).toBe(false);
  });
});

test("untagged union", () => {
  expect(LimitSchema.parse(5)).toBe(5);
  expect(LimitSchema.parse("max")).toBe("max");
  expect(LimitSchema.safeParse(-1).success).toBe(false);
  expect(LimitSchema.safeParse(1.5).success).toBe(false);
});

test("struct constraints", () => {
  const ok = { id: "1", name: "Ada", emails: ["a@x.test"], score: 0.5, avatar: [1, 2], tags: { g: ["a"] } };
  expect(ContactSchema.parse(ok)).toEqual(ok);
  expect(ContactSchema.safeParse({ ...ok, score: 2 }).success).toBe(false);
  expect(ContactSchema.safeParse({ ...ok, avatar: [256] }).success).toBe(false);
  expect(ContactSchema.safeParse({ ...ok, emails: Array(11).fill("a") }).success).toBe(false);
  expect(ContactSchema.safeParse({ id: "1", name: "Ada" }).success).toBe(false);
});

test("key defaults parse with their schemas", () => {
  for (const group of Object.values(keys)) {
    for (const [name, k] of Object.entries(group)) {
      if (k.default !== undefined) {
        expect(k.schema.safeParse(k.default).success, name).toBe(true);
      }
    }
  }
  expect(keys.session["timeout_s"].default).toBe(600);
  expect(keys.session["timeout_s"].schema.safeParse(0).success).toBe(false);
  expect(keys.session["backend"].schema.safeParse("gpt").success).toBe(false);
});

test("the bare types agree with the Zod types", () => {
  const server: Bare.McpServer = { type: "http", url: "https://x.test" };
  expect(McpServerSchema.parse(server)).toEqual(server);
  const defaults: Bare.SessionKeys = {
    "steer.enabled": false,
    backend: "claude",
    timeout_s: 600,
    temperature: 0.7,
    turn_limit: 50,
  };
  expect(sessionDefaults).toEqual(defaults);
});
