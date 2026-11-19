import { fireEvent, render } from "@testing-library/svelte";
import { expect, it } from "vitest";
import PodcastCover from "./PodcastCover.svelte";

it("chooses a crisp Apple thumbnail and retries its published URL when a variant fails", async () => {
  const source = "https://is1-ssl.mzstatic.com/image/thumb/Podcasts/cover.jpg/170x170bb.png";
  const { container, rerender } = render(PodcastCover, { source });
  const img = container.querySelector("img")!;
  expect(img.srcset).toContain("480x480bb.webp 480w");
  await fireEvent.error(img);
  expect(img).not.toHaveAttribute("srcset");
  expect(img).toHaveAttribute("src", source);
  await fireEvent.load(img);
  expect(img).toHaveClass("ready");

  const signed = `${source}?token=publisher`;
  for (const original of [signed, "https://publisher.example/image/thumb/170x170bb.png"]) {
    await rerender({ source: original });
    expect(container.querySelector("img")).toHaveAttribute("src", original);
    expect(container.querySelector("img")).not.toHaveAttribute("srcset");
  }
});
