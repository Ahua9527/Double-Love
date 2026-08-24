import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";

describe("App 项目库（无桌面壳）", () => {
  beforeEach(() => window.localStorage.clear());

  it("渲染左对齐项目库与精简工作区导航", () => {
    render(<App />);
    expect(screen.getByText("项目库还是空的")).toBeTruthy();
    expect(screen.getByRole("button", { name: "新建项目" })).toBeTruthy();
    expect(screen.getByPlaceholderText("搜索项目")).toBeTruthy();
    expect(screen.getByLabelText("工作区导航")).toBeTruthy();
    expect(screen.queryByText("新建转录")).toBeNull();
    expect(screen.getAllByText("我的项目").length).toBeGreaterThan(0);
  });

  it("无桌面壳时导入项目给出说明而不崩溃", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "更多项目操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /导入已有项目/ }));
    expect(
      screen.getByText("请在桌面应用中导入本地项目。"),
    ).toBeTruthy();
  });

  it("项目栏可以收起与恢复", () => {
    render(<App />);
    const toggle = screen.getByLabelText("切换项目栏");
    expect(screen.getByLabelText("工作区导航")).toBeTruthy();
    fireEvent.click(toggle);
    expect(screen.queryByLabelText("工作区导航")).toBeNull();
    fireEvent.click(toggle);
    expect(screen.getByLabelText("工作区导航")).toBeTruthy();
  });

  it("后台任务和设置是独立场景", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "后台任务" }));
    expect(screen.getByText("当前没有后台任务")).toBeTruthy();
    fireEvent.click(screen.getAllByRole("button", { name: "设置" })[0]);
    expect(
      screen.getByText("打开一个本地项目后，可以调整画布、字幕和项目历史。"),
    ).toBeTruthy();
  });
});
