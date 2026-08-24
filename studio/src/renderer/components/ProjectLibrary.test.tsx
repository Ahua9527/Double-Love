import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProjectLibrary } from "./ProjectLibrary";

const projects = Array.from({ length: 25 }, (_, index) => ({
  project_id: `project-${index}`,
  root: `/Volumes/Projects/项目 ${index}`,
  display_name: `项目 ${index}`,
  last_opened_at: `2026-08-${String((index % 20) + 1).padStart(2, "0")}T10:00:00Z`,
  exists: true,
  created_at: "2026-08-01T08:00:00Z",
  modified_at: "2026-08-20T12:00:00Z",
  canvas: { width: 1920n, height: 1080n, background: "#000000", fit: "contain" as const, position_x: 0, position_y: 0, scale: 1, rotation_degrees: 0, opacity: 1 },
  output_rate: "fps_25" as const,
}));

describe("ProjectLibrary", () => {
  it("renders the complete registered list and searches only by project name", () => {
    render(
      <ProjectLibrary
        project={null}
        assets={[]}
        recentProjects={projects}
        view="grid"
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onViewChange={vi.fn()}
        onOpenRecent={vi.fn()}
        onRelocate={vi.fn()}
        onRemoveRecent={vi.fn()}
        onTrash={vi.fn().mockResolvedValue(true)}
      />,
    );
    expect(screen.getByText("25 个项目")).toBeTruthy();
    expect(screen.getByText("项目 24")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("搜索项目"), {
      target: { value: "项目 17" },
    });
    expect(screen.getByText("项目 17")).toBeTruthy();
    expect(screen.queryByText("项目 16")).toBeNull();
    expect(screen.getAllByRole("button", { name: "新建项目" })).toHaveLength(1);
    const header = screen.getByRole("heading", { name: "我的项目" }).closest("header")!;
    expect(within(header).getByLabelText("搜索项目")).toBeTruthy();
    expect(within(header).getByRole("button", { name: "网格视图" })).toBeTruthy();
    expect(document.querySelector(".studio-library-toolbar")).toBeNull();
  });

  it("opens registered projects by their record and offers recovery for missing locations", () => {
    const onOpenRecent = vi.fn();
    const onRelocate = vi.fn();
    const onRemoveRecent = vi.fn().mockResolvedValue(true);
    const missing = { ...projects[0], exists: false };
    render(
      <ProjectLibrary
        project={null}
        assets={[]}
        recentProjects={[projects[1], missing]}
        view="list"
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onViewChange={vi.fn()}
        onOpenRecent={onOpenRecent}
        onRelocate={onRelocate}
        onRemoveRecent={onRemoveRecent}
        onTrash={vi.fn().mockResolvedValue(true)}
      />,
    );
    fireEvent.click(screen.getByText("项目 1").closest("button")!);
    expect(onOpenRecent).toHaveBeenCalledWith(projects[1]);
    fireEvent.click(screen.getByRole("button", { name: "重新定位" }));
    expect(onRelocate).toHaveBeenCalledWith(missing);
    fireEvent.contextMenu(screen.getByText("项目 0").closest("article")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "删除…" }));
    expect(screen.queryByRole("checkbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "从项目库删除" }));
    expect(onRemoveRecent).toHaveBeenCalledWith(missing);
    expect(screen.getAllByText("1920 × 1080")).toHaveLength(2);
    expect(screen.getAllByText("25 fps")).toHaveLength(2);
  });

  it("switches between grid and list views", () => {
    const onViewChange = vi.fn();
    render(
      <ProjectLibrary
        project={null}
        assets={[]}
        recentProjects={projects.slice(0, 1)}
        view="grid"
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onViewChange={onViewChange}
        onOpenRecent={vi.fn()}
        onRelocate={vi.fn()}
        onRemoveRecent={vi.fn()}
        onTrash={vi.fn().mockResolvedValue(true)}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "列表视图" }));
    expect(onViewChange).toHaveBeenCalledWith("list");
  });

  it("uses one delete flow and only trashes files when explicitly checked", async () => {
    const onTrash = vi.fn().mockResolvedValue(true);
    render(
      <ProjectLibrary
        project={null}
        assets={[]}
        recentProjects={projects.slice(0, 1)}
        view="grid"
        onCreate={vi.fn()}
        onImport={vi.fn()}
        onViewChange={vi.fn()}
        onOpenRecent={vi.fn()}
        onRelocate={vi.fn()}
        onRemoveRecent={vi.fn().mockResolvedValue(true)}
        onTrash={onTrash}
      />,
    );
    fireEvent.contextMenu(screen.getByText("项目 0").closest("article")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "删除…" }));
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    expect(screen.getByRole("button", { name: "从项目库删除" })).toBeTruthy();
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "移到废纸篓" }));
    expect(onTrash).toHaveBeenCalledWith(projects[0]);
  });

  it("opens the same delete menu from the keyboard context key", () => {
    render(
      <ProjectLibrary
        project={null} assets={[]} recentProjects={projects.slice(0, 1)} view="grid"
        onCreate={vi.fn()} onImport={vi.fn()} onViewChange={vi.fn()}
        onOpenRecent={vi.fn()} onRelocate={vi.fn()}
        onRemoveRecent={vi.fn().mockResolvedValue(true)}
        onTrash={vi.fn().mockResolvedValue(true)}
      />,
    );
    fireEvent.keyDown(screen.getByText("项目 0").closest("button")!, { key: "F10", shiftKey: true });
    expect(screen.getByRole("menuitem", { name: "删除…" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "项目 0 更多操作" })).toBeNull();
  });

  it("keeps library removal available but disables trash while tasks run", () => {
    render(
      <ProjectLibrary
        project={null} assets={[]} recentProjects={projects.slice(0, 1)} view="grid"
        onCreate={vi.fn()} onImport={vi.fn()} onViewChange={vi.fn()}
        onOpenRecent={vi.fn()} onRelocate={vi.fn()}
        onRemoveRecent={vi.fn().mockResolvedValue(true)}
        onTrash={vi.fn().mockResolvedValue(true)} trashDisabled
      />,
    );
    fireEvent.contextMenu(screen.getByText("项目 0").closest("article")!);
    fireEvent.click(screen.getByRole("menuitem", { name: "删除…" }));
    expect((screen.getByRole("checkbox") as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByRole("button", { name: "从项目库删除" }).hasAttribute("disabled")).toBe(false);
  });
});
