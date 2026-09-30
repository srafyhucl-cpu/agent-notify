import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { opencodeModelsFixture } from "../../test/fixtures";
import { OrcNodeChain, type OrcNodeModelEditing } from "./OrcNodeChain";

/** 行内编辑目标步骤：OpenCode + 已保存模型。 */
const step = {
  order: 1,
  role: "executor",
  agent: "opencode",
  model: "opencode-go/deepseek-v4.1-flash",
  variant: null,
};

function renderInlineEditing(saving: boolean) {
  const onSave = vi.fn();
  const editing: OrcNodeModelEditing = {
    models: opencodeModelsFixture(),
    saving,
    onSave,
  };
  const view = render(
    <OrcNodeChain label="工作流节点" steps={[step]} modelEditing={editing} />,
  );
  return { onSave, ...view };
}

describe("OrcNodeChain 内联模型/强度保存（B5）", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("连续切换模型只触发一次保存，且以最后一次为准", () => {
    vi.useFakeTimers();
    const { onSave } = renderInlineEditing(false);
    const modelSelect = screen.getByLabelText("第 1 步 模型");

    // 防抖窗口内连续切换：重排 timer，不逐次发请求。
    fireEvent.change(modelSelect, {
      target: { value: "opencode-go/space-bunny-free" },
    });
    fireEvent.change(modelSelect, {
      target: { value: "opencode/mimo-v2.6-flash" },
    });
    fireEvent.change(modelSelect, { target: { value: "" } });
    expect(onSave).not.toHaveBeenCalled();

    vi.advanceTimersByTime(300);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(1, "", "");
  });

  it("连续切换思考强度只触发一次保存，且以最后一次为准", () => {
    vi.useFakeTimers();
    const { onSave } = renderInlineEditing(false);
    const variantSelect = screen.getByLabelText("第 1 步 思考强度");

    fireEvent.change(variantSelect, { target: { value: "high" } });
    fireEvent.change(variantSelect, { target: { value: "max" } });
    expect(onSave).not.toHaveBeenCalled();

    vi.advanceTimersByTime(300);
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(
      1,
      "opencode-go/deepseek-v4.1-flash",
      "max",
    );
  });

  it("未到防抖窗口就卸载：丢弃未发出的保存（不泄漏 timer）", () => {
    vi.useFakeTimers();
    const { onSave, unmount } = renderInlineEditing(false);
    fireEvent.change(screen.getByLabelText("第 1 步 模型"), {
      target: { value: "opencode-go/space-bunny-free" },
    });

    unmount();
    vi.advanceTimersByTime(300);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("保存中禁用模型与强度下拉，避免并发写入", () => {
    renderInlineEditing(true);
    expect(screen.getByLabelText("第 1 步 模型")).toBeDisabled();
    expect(screen.getByLabelText("第 1 步 思考强度")).toBeDisabled();
  });
});
