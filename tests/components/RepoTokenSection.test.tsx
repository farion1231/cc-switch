import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import { RepoTokenSection } from "@/components/skills/RepoTokenSection";
import { tokenHostFromInput } from "@/components/skills/repoUrl";

// #9666：访问令牌按站点存，输入框常被贴进整条仓库地址
describe("tokenHostFromInput", () => {
  it("takes the host, with its port, out of whatever was pasted", () => {
    expect(tokenHostFromInput(" GitLab.Example.com ")).toBe(
      "gitlab.example.com",
    );
    expect(
      tokenHostFromInput("https://gitlab.example.com:8443/group/sub/project"),
    ).toBe("gitlab.example.com:8443");
    expect(tokenHostFromInput("gitlab.example.com/group/project")).toBe(
      "gitlab.example.com",
    );
    expect(tokenHostFromInput("git@gitlab.example.com:group/x.git")).toBe(
      "gitlab.example.com",
    );
    // ssh:// 的端口是 SSH 的，下载按 https 默认端口找令牌
    expect(
      tokenHostFromInput("ssh://git@gitlab.example.com:2222/group/repo.git"),
    ).toBe("gitlab.example.com");
    expect(tokenHostFromInput("https://www.github.com/owner/repo")).toBe(
      "github.com",
    );
    expect(tokenHostFromInput("")).toBe("");
  });
});

function renderSection(
  overrides: Partial<Parameters<typeof RepoTokenSection>[0]> = {},
) {
  const props = {
    hosts: [],
    secureStorage: true,
    knownHosts: [],
    onSave: vi.fn().mockResolvedValue("ok"),
    onRemove: vi.fn().mockResolvedValue("ok"),
    ...overrides,
  };
  render(<RepoTokenSection {...props} />);
  return props;
}

describe("RepoTokenSection", () => {
  it("saves the token under the host taken from a pasted URL", async () => {
    const props = renderSection();
    fireEvent.change(screen.getByLabelText("skills.repo.token.host"), {
      target: { value: "https://GitLab.example.com:8443/group/project" },
    });
    const tokenInput = screen.getByLabelText("skills.repo.token.value");
    expect(tokenInput).toHaveAttribute("type", "password");
    fireEvent.change(tokenInput, { target: { value: "glpat-abc" } });
    fireEvent.click(screen.getByRole("button", { name: /token\.save/ }));

    await waitFor(() =>
      expect(props.onSave).toHaveBeenCalledWith(
        "gitlab.example.com:8443",
        "glpat-abc",
      ),
    );
    // 存好后清空，令牌不留在输入框里
    await waitFor(() => expect(tokenInput).toHaveValue(""));
  });

  it("asks for both fields before saving", () => {
    const props = renderSection();
    fireEvent.change(screen.getByLabelText("skills.repo.token.host"), {
      target: { value: "github.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: /token\.save/ }));
    expect(props.onSave).not.toHaveBeenCalled();
    expect(
      screen.getByText("skills.repo.token.missingFields"),
    ).toBeInTheDocument();
  });

  it("shows the backend's structured error", async () => {
    renderSection({
      onSave: vi.fn().mockRejectedValue(
        JSON.stringify({
          code: "INVALID_REPO_TOKEN",
          context: { limit: "1024" },
        }),
      ),
    });
    fireEvent.change(screen.getByLabelText("skills.repo.token.host"), {
      target: { value: "github.com" },
    });
    fireEvent.change(screen.getByLabelText("skills.repo.token.value"), {
      target: { value: "bad token" },
    });
    fireEvent.click(screen.getByRole("button", { name: /token\.save/ }));
    expect(
      await screen.findByText("skills.error.invalidRepoToken"),
    ).toBeInTheDocument();
  });

  it("lists configured hosts and removes one", async () => {
    const props = renderSection({ hosts: ["gitlab.example.com"] });
    expect(screen.getByText("gitlab.example.com")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "skills.repo.token.removeAria" }),
    );
    await waitFor(() =>
      expect(props.onRemove).toHaveBeenCalledWith("gitlab.example.com"),
    );
  });

  it("says where the token is kept", () => {
    renderSection({ secureStorage: false });
    expect(
      screen.getByText("skills.repo.token.descriptionFile"),
    ).toBeInTheDocument();
  });
});

describe("RepoManagerPanel token section", () => {
  const repo = {
    host: "gitlab.example.com",
    owner: "group",
    name: "skills",
    branch: "main",
    enabled: true,
  };

  it("is hidden unless the caller wires tokens in", () => {
    render(
      <RepoManagerPanel
        repos={[repo]}
        skills={[]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.queryByText("skills.repo.token.title")).toBeNull();
  });

  it("offers the hosts of added repositories as site suggestions", () => {
    const { container } = render(
      <RepoManagerPanel
        repos={[repo, { ...repo, host: undefined, owner: "o", name: "r" }]}
        skills={[]}
        onAdd={vi.fn()}
        onRemove={vi.fn()}
        tokens={{
          hosts: ["github.com"],
          secureStorage: true,
          onSave: vi.fn(),
          onRemove: vi.fn(),
        }}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("skills.repo.token.title")).toBeInTheDocument();
    // 已设置令牌的 github.com 不再出现在候选里
    const options = Array.from(
      (container.ownerDocument ?? document).querySelectorAll(
        "#sk-repo-token-hosts option",
      ),
    ).map((option) => option.getAttribute("value"));
    expect(options).toEqual(["gitlab.example.com"]);
  });
});
