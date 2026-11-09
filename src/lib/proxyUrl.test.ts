import { describe, expect, it } from "vitest";
import {
  isValidProxyUrl,
  joinProxyUrl,
  splitProxyUrl,
  withProviderProxy,
} from "./proxyUrl";

describe("isValidProxyUrl", () => {
  it("accepts supported schemes", () => {
    expect(isValidProxyUrl("http://127.0.0.1:7890")).toBe(true);
    expect(isValidProxyUrl("https://proxy.example.com")).toBe(true);
    expect(isValidProxyUrl("socks5://user:pass@127.0.0.1:1080")).toBe(true);
    expect(isValidProxyUrl(" socks5h://127.0.0.1:1080 ")).toBe(true);
  });

  it("rejects unsupported or malformed values", () => {
    expect(isValidProxyUrl("127.0.0.1:7890")).toBe(false);
    expect(isValidProxyUrl("ftp://127.0.0.1:21")).toBe(false);
    expect(isValidProxyUrl("socks4://127.0.0.1:1080")).toBe(false);
    expect(isValidProxyUrl("")).toBe(false);
  });
});

describe("splitProxyUrl / joinProxyUrl", () => {
  it("splits credentials without adding a root slash", () => {
    expect(splitProxyUrl("http://user:p%40ss@127.0.0.1:7890")).toEqual({
      baseUrl: "http://127.0.0.1:7890",
      username: "user",
      password: "p@ss",
    });
    expect(splitProxyUrl("")).toEqual({
      baseUrl: "",
      username: "",
      password: "",
    });
  });

  it("joins credentials back without adding a root slash", () => {
    expect(joinProxyUrl("socks5://127.0.0.1:1080", "user", "p@ss")).toBe(
      "socks5://user:p%40ss@127.0.0.1:1080",
    );
    expect(joinProxyUrl("http://127.0.0.1:7890", "", "")).toBe(
      "http://127.0.0.1:7890",
    );
    expect(joinProxyUrl("", "user", "pass")).toBe("");
  });

  it("round-trips", () => {
    const url = "http://user:secret@proxy.example.com:8080";
    const { baseUrl, username, password } = splitProxyUrl(url);
    expect(joinProxyUrl(baseUrl, username, password)).toBe(url);
  });
});

describe("withProviderProxy", () => {
  it("sets a trimmed proxy and keeps other meta fields", () => {
    expect(
      withProviderProxy({ isPartner: true }, " http://127.0.0.1:7890 "),
    ).toEqual({ isPartner: true, proxyUrl: "http://127.0.0.1:7890" });
  });

  it("removes a cleared proxy", () => {
    expect(
      withProviderProxy(
        { isPartner: true, proxyUrl: "http://127.0.0.1:7890" },
        "  ",
      ),
    ).toEqual({ isPartner: true });
  });

  it("leaves missing meta alone when there is no proxy", () => {
    expect(withProviderProxy(undefined, "")).toBeUndefined();
  });
});
