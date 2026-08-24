import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { projectNameError } from "../project-name";
import { NewProjectDialog } from "./NewProjectDialog";

describe("NewProjectDialog", () => {
  it.each(["", "  ", "采访/第一天", "采访\\第一天", ".", ".."])(
    "rejects invalid name %j",
    (name) => expect(projectNameError(name)).not.toBeNull(),
  );

  it("trims Chinese names and exposes no location controls", () => {
    const onCreate = vi.fn();
    render(
      <NewProjectDialog
        onCreate={onCreate}
        onClose={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText("项目名称"), {
      target: { value: "  春日采访  " },
    });
    expect(screen.queryByText("保存位置")).toBeNull();
    expect(screen.queryByRole("button", { name: "更改位置" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "创建项目" }));
    expect(onCreate).toHaveBeenCalledWith({
      name: "春日采访",
    });
  });
});
