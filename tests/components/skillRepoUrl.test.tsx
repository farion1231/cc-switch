import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RepoManagerPanel } from "@/components/skills/RepoManagerPanel";
import {
  isInsecureRepoUrl,
  parseRepoUrl,
  repoWebUrl,
} from "@/components/skills/repoUrl";

describe("parseRepoUrl", () => {
  it("keeps GitHub inputs free of a host", () => {
    for (const input of [
      "owner/name",
      "https://github.com/owner/name",
      "https://github.com/owner/name.git",
      "https://github.com/owner/name/",
      "http://github.com/owner/name",
      "https://www.github.com/owner/name",
      "git@github.com:owner/name.git",
    ]) {
      expect(parseRepoUrl(input), input).toEqual({
        owner: "owner",
        name: "name",
      });
    }
  });

  it("rejects GitHub inputs that are not a repository root", () => {
    for (const input of [
      "",
      "owner",
      "owner/name/extra",
      "https://github.com/owner",
      "https://github.com/owner/name/tree/main",
      "gitlab.com/group/project",
    ]) {
      expect(parseRepoUrl(input), input).toBeNull();
    }
  });

  it("reads nested GitLab groups", () => {
    expect(parseRepoUrl("https://gitlab.com/group/project")).toEqual({
      host: "gitlab.com",
      owner: "group",
      name: "project",
    });
    expect(
      parseRepoUrl("https://GitLab.com/group/sub/deeper/project.git"),
    ).toEqual({
      host: "gitlab.com",
      owner: "group/sub/deeper",
      name: "project",
    });
    expect(parseRepoUrl("git@gitlab.com:group/sub/project.git")).toEqual({
      host: "gitlab.com",
      owner: "group/sub",
      name: "project",
    });
    // ssh:// 写法可以带 SSH 端口，它不是分组，也不是 https 的端口
    for (const input of [
      "ssh://git@gitlab.example.com:2222/group/sub/project.git",
      "ssh://git@gitlab.example.com/group/sub/project.git",
    ]) {
      expect(parseRepoUrl(input), input).toEqual({
        host: "gitlab.example.com",
        owner: "group/sub",
        name: "project",
      });
    }
    // 界面地址里 `/-/` 之后的部分不属于项目路径
    expect(
      parseRepoUrl("https://gitlab.com/group/project/-/tree/main/skills"),
    ).toEqual({ host: "gitlab.com", owner: "group", name: "project" });
    expect(parseRepoUrl("https://gitlab.com/project")).toBeNull();
  });

  it("reads Gitea and Forgejo addresses, including page URLs", () => {
    const expected = { host: "codeberg.org", owner: "team", name: "skills" };
    expect(parseRepoUrl("https://codeberg.org/team/skills")).toEqual(expected);
    expect(parseRepoUrl("https://codeberg.org/team/skills.git")).toEqual(
      expected,
    );
    // 页面地址里仓库只有前两段，`src/branch/…` 之后是界面路径
    for (const tail of [
      "src/branch/main",
      "src/branch/feature/x/skills/demo",
      "src/tag/v1.0.0",
      "src/commit/0123abcd/README.md",
    ]) {
      expect(parseRepoUrl(`https://codeberg.org/team/skills/${tail}`)).toEqual(
        expected,
      );
    }
    expect(
      parseRepoUrl("https://git.example.com:3000/team/skills/src/branch/main"),
    ).toEqual({ host: "git.example.com:3000", owner: "team", name: "skills" });
    // 只有 `src` 后面跟着 branch / tag / commit 才算页面路径：GitLab 的嵌套分组可以叫 src
    expect(parseRepoUrl("https://gitlab.com/group/sub/src/project")).toEqual({
      host: "gitlab.com",
      owner: "group/sub/src",
      name: "project",
    });
    // 克隆地址和带 `/-/` 的 GitLab 页面地址不是 Gitea 页面，分组恰好叫 src/branch 也不截断
    const nested = {
      host: "gitlab.com",
      owner: "group/sub/src/branch",
      name: "skills",
    };
    for (const url of [
      "https://gitlab.com/group/sub/src/branch/skills.git",
      "https://gitlab.com/group/sub/src/branch/skills/-/tree/main",
      "git@gitlab.com:group/sub/src/branch/skills.git",
      "ssh://git@gitlab.com/group/sub/src/branch/skills.git",
    ]) {
      expect(parseRepoUrl(url)).toEqual(nested);
    }
  });

  it("keeps the port of a self-hosted instance", () => {
    // #8389 报告人的坐标：裸 IP、非默认端口、分组名带下划线
    expect(
      parseRepoUrl("https://10.22.36.105:8081/me_release/SkillList"),
    ).toEqual({
      host: "10.22.36.105:8081",
      owner: "me_release",
      name: "SkillList",
    });
    expect(
      repoWebUrl({
        host: "gitlab.example.com:8443",
        owner: "team/ai",
        name: "skills",
      }),
    ).toBe("https://gitlab.example.com:8443/team/ai/skills");
    expect(repoWebUrl({ owner: "owner", name: "name" })).toBe(
      "https://github.com/owner/name",
    );
  });
});

describe("isInsecureRepoUrl", () => {
  it("flags plain-http GitLab addresses only", () => {
    expect(
      isInsecureRepoUrl("http://10.22.36.105:8081/me_release/Skills"),
    ).toBe(true);
    expect(isInsecureRepoUrl("https://gitlab.com/group/project")).toBe(false);
    // GitHub 的 http 写法一直被接受，下载本来就走 https
    expect(isInsecureRepoUrl("http://github.com/owner/name")).toBe(false);
    expect(isInsecureRepoUrl("owner/name")).toBe(false);
  });
});

describe("RepoManagerPanel add form", () => {
  const renderPanel = (onAdd = vi.fn().mockResolvedValue(undefined)) => {
    render(
      <RepoManagerPanel
        repos={[]}
        skills={[]}
        onAdd={onAdd}
        onRemove={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    return onAdd;
  };
  const submit = (url: string) => {
    fireEvent.change(
      screen.getByPlaceholderText("skills.repo.urlPlaceholder"),
      { target: { value: url } },
    );
    fireEvent.click(screen.getByRole("button", { name: /skills\.repo\.add/ }));
  };

  it("passes the host of a GitLab repository to onAdd", async () => {
    const onAdd = renderPanel();
    submit("https://gitlab.example.com:8443/group/sub/skills");
    await waitFor(() =>
      expect(onAdd).toHaveBeenCalledWith({
        host: "gitlab.example.com:8443",
        owner: "group/sub",
        name: "skills",
        branch: "main",
        enabled: true,
      }),
    );
  });

  it("adds a GitHub repository without a host", async () => {
    const onAdd = renderPanel();
    submit("https://github.com/owner/name");
    await waitFor(() =>
      expect(onAdd).toHaveBeenCalledWith({
        owner: "owner",
        name: "name",
        branch: "main",
        enabled: true,
      }),
    );
  });

  it("explains that plain-http GitLab addresses are not supported", () => {
    const onAdd = renderPanel();
    submit("http://10.22.36.105:8081/me_release/SkillList");
    expect(screen.getByText("skills.repo.httpsOnly")).toBeInTheDocument();
    expect(onAdd).not.toHaveBeenCalled();
  });
});
