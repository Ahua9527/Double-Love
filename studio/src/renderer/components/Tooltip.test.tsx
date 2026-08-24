import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Tooltip } from "./Tooltip";

afterEach(() => vi.useRealTimers());

describe("Tooltip", () => {
  it("appears after the hover delay and closes on escape", () => {
    vi.useFakeTimers();
    render(<Tooltip label="添加素材"><button type="button">+</button></Tooltip>);
    fireEvent.mouseEnter(screen.getByRole("button"));
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => vi.advanceTimersByTime(400));
    expect(screen.getByRole("tooltip").textContent).toBe("添加素材");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("also appears for keyboard focus", () => {
    vi.useFakeTimers();
    render(<Tooltip label="导出项目"><button type="button">↑</button></Tooltip>);
    fireEvent.focus(screen.getByRole("button"));
    act(() => vi.advanceTimersByTime(400));
    expect(screen.getByRole("tooltip")).toBeTruthy();
  });
});
