import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { NumericInput } from "./numeric-input";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Width/height pair linked 2:1, like the store's preserved aspect ratio.
function LinkedPair() {
  const [dims, setDims] = useState({ w: 100, h: 50 });
  return (
    <>
      <NumericInput
        id="w"
        min={1}
        value={dims.w}
        onCommit={(w) => setDims({ w, h: Math.round(w / 2) })}
      />
      <NumericInput id="h" min={1} value={dims.h} onCommit={(h) => setDims({ w: h * 2, h })} />
    </>
  );
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => root.render(<LinkedPair />));
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const input = (id: string) => container.querySelector<HTMLInputElement>(`#${id}`)!;
const values = () => [input("w").value, input("h").value];

const focus = (el: HTMLInputElement) => act(() => el.focus());
const blur = (el: HTMLInputElement) => act(() => el.blur());
const key = (el: HTMLInputElement, k: string) =>
  act(() => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  });
const type = (el: HTMLInputElement, text: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, text);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
const spin = (el: HTMLInputElement, dir: "up" | "down") =>
  act(() => {
    const [up, down] = el.parentElement!.querySelectorAll("button");
    (dir === "up" ? up : down).dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    document.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
  });

describe("NumericInput in a linked pair", () => {
  it("arrow keys update the focused field, not just its partner", () => {
    focus(input("w"));
    key(input("w"), "ArrowUp");
    expect(values()).toEqual(["101", "51"]);
    key(input("w"), "ArrowDown");
    key(input("w"), "ArrowDown");
    expect(values()).toEqual(["99", "50"]);
  });

  it("blur after arrow keys does not revert to the pre-arrow value", () => {
    focus(input("h"));
    key(input("h"), "ArrowUp");
    blur(input("h"));
    expect(values()).toEqual(["102", "51"]);
  });

  it("arrow keys nudge from the typed, uncommitted text", () => {
    focus(input("w"));
    type(input("w"), "200");
    key(input("w"), "ArrowUp");
    expect(values()).toEqual(["201", "101"]);
    blur(input("w"));
    expect(values()).toEqual(["201", "101"]);
  });

  it("spinner buttons update a focused field", () => {
    focus(input("w"));
    spin(input("w"), "up");
    expect(values()).toEqual(["101", "51"]);
    blur(input("w"));
    expect(values()).toEqual(["101", "51"]);
  });

  it("clamps to min", () => {
    focus(input("h"));
    type(input("h"), "1");
    key(input("h"), "ArrowDown");
    expect(values()).toEqual(["2", "1"]);
  });

  it("Escape discards the typed text", () => {
    focus(input("w"));
    type(input("w"), "300");
    key(input("w"), "Escape");
    expect(values()).toEqual(["100", "50"]);
  });
});
