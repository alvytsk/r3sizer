import { describe, expect, it } from "vitest";
import { highlightJson } from "./json-viewer";

describe("highlightJson", () => {
  it("escapes markup inside string values", () => {
    const html = highlightJson(JSON.stringify({ name: "<img src=x onerror=alert(1)>&" }));
    expect(html).not.toContain("<img");
    expect(html).toContain("&lt;img src=x onerror=alert(1)&gt;&amp;");
  });
});
