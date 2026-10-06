/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { shellVars } from "./layout";

const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

function rule(selector: string): string {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = new RegExp(`(?:^|})\\s*${esc}\\s*\\{([^}]*)\\}`, "m").exec(css);
  if (m === null || m[1] === undefined) throw new Error(`rule ${selector} not found in styles.css`);
  return m[1];
}

function decl(body: string, prop: string): string | undefined {
  const m = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`, "m").exec(body);
  return m?.[1]?.trim();
}

/** Every custom property the assertions below expect the stylesheet to read. */
const ASSERTED_VARS = ["--gutter", "--row-h", "--row-pad-x", "--grid-gap", "--panel-border"] as const;

describe("styles.css reads the shell variables (D9)", () => {
  it("the stylesheet was read", () => {
    expect(css.length).toBeGreaterThan(1000);
  });
  it("every asserted variable is a key of shellVars()", () => {
    const keys = Object.keys(shellVars());
    for (const v of ASSERTED_VARS) expect(keys).toContain(v);
  });
  it(".app padding is the gutter", () => {
    expect(decl(rule(".app"), "padding")).toBe("var(--gutter)");
  });
  it(".app-inner gap is the gutter and it declares no max-width", () => {
    const body = rule(".app-inner");
    expect(decl(body, "gap")).toBe("var(--gutter)");
    expect(decl(body, "max-width")).toBeUndefined();
  });
  it(".row-grid takes height, padding and gap from the variables", () => {
    const body = rule(".row-grid");
    expect(decl(body, "height")).toBe("var(--row-h)");
    expect(decl(body, "padding")).toBe("0 var(--row-pad-x)");
    expect(decl(body, "gap")).toBe("var(--grid-gap)");
  });
  it(".thead lines its tracks up with the rows", () => {
    const body = rule(".thead");
    expect(decl(body, "gap")).toBe("var(--grid-gap)");
    expect(decl(body, "padding")).toContain("var(--row-pad-x)");
  });
  it(".panel border is the panel-border variable", () => {
    expect(decl(rule(".panel"), "border")).toContain("var(--panel-border)");
  });
});
