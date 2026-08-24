import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";

describe("Sidebar task badge", () => {
  it("hides a zero count completely", () => {
    render(
      <Sidebar
        screen="library"
        taskCount={0}
        taskTone="attention"
        onNavigate={vi.fn()}
      />,
    );
    expect(screen.queryByLabelText(/个后台任务/)).toBeNull();
    expect(screen.queryByText("新建转录")).toBeNull();
    expect(screen.queryByText("打开已有项目")).toBeNull();
    expect(screen.queryByText(/DOUBLE LOVE/)).toBeNull();
  });

  it("renders a blue numeric badge for active work", () => {
    render(
      <Sidebar
        screen="library"
        taskCount={3}
        taskTone="active"
        onNavigate={vi.fn()}
      />,
    );
    const badge = screen.getByLabelText("3 个后台任务");
    expect(badge.textContent).toBe("3");
    expect(badge.className).toContain("is-active");
  });

  it("renders an amber badge when only attention items remain", () => {
    render(
      <Sidebar
        screen="tasks"
        taskCount={2}
        taskTone="attention"
        onNavigate={vi.fn()}
      />,
    );
    expect(screen.getByLabelText("2 个后台任务").className).toContain(
      "is-attention",
    );
  });
});
