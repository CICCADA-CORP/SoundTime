import { fireEvent, render } from "@testing-library/svelte";
import { expect, it } from "vitest";
import StationIcon from "./StationIcon.svelte";

it("shows an antenna until the current logo loads and keeps it for failed or missing logos", async () => {
  const { container, rerender } = render(StationIcon, { source: "https://radio.example/first.png" });
  const first = container.querySelector("img")!;
  expect(container.querySelector("svg")).toBeInTheDocument();
  expect(first).not.toHaveAttribute("data-loaded");
  await fireEvent.load(first);
  expect(first).toHaveAttribute("data-loaded");

  await rerender({ source: "https://radio.example/second.png" });
  const second = container.querySelector("img")!;
  expect(second).not.toBe(first);
  expect(second).not.toHaveAttribute("data-loaded");
  await fireEvent.load(first);
  expect(second).not.toHaveAttribute("data-loaded");
  await fireEvent.load(second);
  await fireEvent.error(second);
  expect(second).not.toHaveAttribute("data-loaded");
  expect(container.querySelector("svg")).toBeInTheDocument();

  await rerender({ source: null });
  expect(container.querySelector("img")).toBeNull();
  expect(container.querySelector("svg")).toBeInTheDocument();
});
