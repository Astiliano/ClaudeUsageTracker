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
const ASSERTED_VARS = ["--gutter", "--row-h", "--row-pad-x", "--grid-gap", "--panel-border", "--ring-min", "--ring-max"] as const;

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
  it(".panel has no border-radius (flat look)", () => {
    expect(decl(rule(".panel"), "border-radius")).toBe("0");
  });
  it(".card has no border-radius (flat look)", () => {
    expect(decl(rule(".card"), "border-radius")).toBe("0");
  });
  it(".chart has no border-radius (flat look)", () => {
    expect(decl(rule(".chart"), "border-radius")).toBe("0");
  });
  it(".chart declares no height (useChartHeight sizes it inline)", () => {
    expect(decl(rule(".chart"), "height")).toBeUndefined();
  });
  it(".banner has no border-radius (flat look)", () => {
    expect(decl(rule(".banner"), "border-radius")).toBe("0");
  });
  it(".modal-body has no border-radius (flat look)", () => {
    expect(decl(rule(".modal-body"), "border-radius")).toBe("0");
  });
  it(".ring width clamps between the ring variables", () => {
    const w = decl(rule(".ring"), "width");
    expect(w).toContain("var(--ring-min)");
    expect(w).toContain("var(--ring-max)");
  });
  it(".card-rings is an inline-size container", () => {
    expect(decl(rule(".card-rings"), "container-type")).toBe("inline-size");
  });
  it(".ring-label may use the full ring width", () => {
    expect(decl(rule(".ring-label"), "max-width")).toBe("100%");
  });
  it(".ring-sm is a fixed 20px glyph", () => {
    const body = rule(".ring-sm");
    expect(decl(body, "width")).toBe("20px");
    expect(decl(body, "height")).toBe("20px");
  });
  it(".ring svg fills its wrapper", () => {
    const body = rule(".ring svg, .ring-sm svg");
    expect(decl(body, "width")).toBe("100%");
    expect(decl(body, "height")).toBe("auto");
    expect(decl(body, "aspect-ratio")).toBe("1");
  });
  it(".spark fills its grid cell", () => {
    expect(decl(rule(".spark"), "width")).toBe("100%");
  });
});
