import { describe, expect, it } from "vitest";
import { parseMcpPort } from "../mcp-settings";

describe("parseMcpPort", () => {
  it("accepts a port in range", () => {
    expect(parseMcpPort(" 51234 ")).toBe(51234);
    expect(parseMcpPort("65535")).toBe(65535);
  });

  it("rejects anything a socket cannot bind", () => {
    expect(parseMcpPort("0")).toBeNull();
    expect(parseMcpPort("65536")).toBeNull();
    expect(parseMcpPort("80.5")).toBeNull();
    expect(parseMcpPort("")).toBeNull();
    expect(parseMcpPort("port")).toBeNull();
  });
});
