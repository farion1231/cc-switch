import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HuaweiQuotaUsage } from "@/huawei/HuaweiQuotaUsage";
import { renderWithQueryClient } from "../utils/testQueryClient";

const api = vi.hoisted(() => ({
  models: vi.fn(),
  quota: vi.fn(),
  modelQuota: vi.fn(),
}));
vi.mock("@/huawei/huaweiCodeAgent2Api", () => ({
  fetchHuaweiCodeAgent2Models: api.models,
  fetchHuaweiQuota: api.quota,
  fetchHuaweiModelQuota: api.modelQuota,
}));
const report = {
  quotas: [
    {
      type: "TPD",
      used_quota: "557",
      total_quota: "400000000",
      remaining_quota: "399999443",
      reset_at: "2026-10-24 00:00:00",
    },
  ],
};
async function selectModel(id = "glm") {
  fireEvent.click(screen.getByRole("button", { name: "加载可查询模型" }));
  const select = await screen.findByLabelText("查询用量的模型");
  fireEvent.change(select, { target: { value: id } });
  fireEvent.click(screen.getByRole("button", { name: "查询模型用量" }));
}
beforeEach(() => {
  vi.resetAllMocks();
  api.models.mockResolvedValue([{ id: "glm" }, { id: "deepseek" }]);
  api.modelQuota.mockResolvedValue(report);
});
describe("Huawei model quota", () => {
  it("uses the selected account/model and displays token quota, remaining and reset", async () => {
    renderWithQueryClient(<HuaweiQuotaUsage accountId="account-a" />);
    await selectModel();
    await screen.findByText("557 / 400,000,000");
    expect(api.models).toHaveBeenCalledWith("account-a");
    expect(api.modelQuota).toHaveBeenCalledWith("account-a", "glm");
    expect(screen.getByText("剩余配额: 399,999,443")).toBeInTheDocument();
    expect(
      screen.getByText("重置时间: 2026-10-24 00:00:00"),
    ).toBeInTheDocument();
    expect(api.quota).not.toHaveBeenCalled();
  });
  it("does not show a late response under another model or account", async () => {
    let resolve!: (value: typeof report) => void;
    api.modelQuota.mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const view = renderWithQueryClient(
      <HuaweiQuotaUsage accountId="account-a" />,
    );
    await selectModel();
    fireEvent.change(screen.getByLabelText("查询用量的模型"), {
      target: { value: "deepseek" },
    });
    resolve(report);
    await waitFor(() =>
      expect(screen.queryByText("557 / 400,000,000")).not.toBeInTheDocument(),
    );
    view.rerender(<HuaweiQuotaUsage accountId="account-b" />);
    expect(screen.queryByLabelText("查询用量的模型")).not.toBeInTheDocument();
    await selectModel("deepseek");
    await screen.findByText("557 / 400,000,000");
    expect(api.modelQuota).toHaveBeenLastCalledWith("account-b", "deepseek");
  });
  it("shows permission failures and empty quotas without fabricating usage", async () => {
    api.modelQuota.mockRejectedValueOnce(new Error("HTTP 403"));
    renderWithQueryClient(<HuaweiQuotaUsage accountId="account-a" />);
    await selectModel();
    expect(await screen.findByRole("alert")).toHaveTextContent("HTTP 403");
    api.modelQuota.mockResolvedValueOnce({ quotas: [] });
    fireEvent.click(screen.getByRole("button", { name: "查询模型用量" }));
    await screen.findByText("该模型暂无配额数据");
    expect(screen.queryByText("557 / 400,000,000")).not.toBeInTheDocument();
  });
});
