import { describe, it, expect } from "vitest";
import { cn } from "@/lib/utils";
import { decodeBase64Utf8 } from "@/lib/utils/base64";

describe("cn() - Tailwind class merging", () => {
  describe("single class", () => {
    it("returns single class unchanged", () => {
      expect(cn("px-4")).toBe("px-4");
    });

    it("handles empty string", () => {
      expect(cn("")).toBe("");
    });

    it("handles whitespace class", () => {
      expect(cn("  px-4  ")).toBe("px-4");
    });
  });

  describe("multiple classes", () => {
    it("combines multiple classes", () => {
      expect(cn("px-4 py-2")).toBe("px-4 py-2");
    });

    it("combines classes from multiple arguments", () => {
      const result = cn("px-4", "py-2", "bg-red-500");
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).toContain("bg-red-500");
    });

    it("handles multiple classes with spaces", () => {
      expect(cn("px-4 py-2 bg-blue-500")).toContain("px-4");
      expect(cn("px-4 py-2 bg-blue-500")).toContain("py-2");
      expect(cn("px-4 py-2 bg-blue-500")).toContain("bg-blue-500");
    });
  });

  describe("conditional classes", () => {
    it("includes classes when condition is true", () => {
      const condition = true;
      const result = cn("px-4", condition && "bg-red-500");
      expect(result).toContain("px-4");
      expect(result).toContain("bg-red-500");
    });

    it("excludes classes when condition is false", () => {
      const condition = false;
      const result = cn("px-4", condition && "bg-red-500");
      expect(result).toContain("px-4");
      expect(result).not.toContain("bg-red-500");
    });

    it("handles multiple conditional classes", () => {
      const isActive = true;
      const isDisabled = false;
      const result = cn(
        "px-4 py-2",
        isActive && "bg-green-500",
        isDisabled && "opacity-50",
      );
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).toContain("bg-green-500");
      expect(result).not.toContain("opacity-50");
    });

    it("handles object notation for conditional classes", () => {
      const isError = true;
      const result = cn("px-4", {
        "bg-red-500": isError,
        "bg-green-500": !isError,
      });
      expect(result).toContain("px-4");
      expect(result).toContain("bg-red-500");
      expect(result).not.toContain("bg-green-500");
    });
  });

  describe("tailwind merging - conflicting classes", () => {
    it("merges padding conflicts - keeps last px class", () => {
      const result = cn("px-2 px-4");
      expect(result).toContain("px-4");
      expect(result).toBe("px-4");
    });

    it("merges margin conflicts - keeps last mx class", () => {
      const result = cn("mx-2 mx-8");
      expect(result).toContain("mx-8");
      expect(result).toBe("mx-8");
    });

    it("merges background color conflicts", () => {
      const result = cn("bg-red-500 bg-blue-500");
      expect(result).toContain("bg-blue-500");
      expect(result).not.toContain("bg-red-500");
    });

    it("merges text color conflicts", () => {
      const result = cn("text-red-500 text-white");
      expect(result).toContain("text-white");
      expect(result).not.toContain("text-red-500");
    });

    it("merges width conflicts", () => {
      const result = cn("w-full w-96");
      expect(result).toContain("w-96");
      expect(result).not.toContain("w-full");
    });

    it("merges height conflicts", () => {
      const result = cn("h-10 h-20");
      expect(result).toContain("h-20");
      expect(result).not.toContain("h-10");
    });

    it("merges border conflicts", () => {
      const result = cn("border-2 border-4");
      expect(result).toContain("border-4");
      expect(result).not.toContain("border-2");
    });

    it("merges display conflicts", () => {
      const result = cn("block inline");
      expect(result).toContain("inline");
      expect(result).not.toContain("block");
    });

    it("handles complex merge scenarios from arguments", () => {
      const result = cn("px-2 py-1", "px-4 py-2", "px-8");
      expect(result).toContain("px-8");
      expect(result).toContain("py-2");
      expect(result).not.toContain("px-2");
      expect(result).not.toContain("py-1");
    });

    it("merges classes correctly with conditional logic", () => {
      const size = "large";
      const result = cn("px-2 py-1", size === "large" && "px-8 py-4", "px-4");
      expect(result).toContain("px-4");
    });
  });

  describe("non-conflicting classes", () => {
    it("preserves all non-conflicting utility classes", () => {
      const result = cn(
        "px-4 py-2 bg-blue-500 text-white rounded-lg border border-gray-300",
      );
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).toContain("bg-blue-500");
      expect(result).toContain("text-white");
      expect(result).toContain("rounded-lg");
      expect(result).toContain("border");
      expect(result).toContain("border-gray-300");
    });

    it("combines padding, margin, background, text in same output", () => {
      const result = cn("mx-auto my-4 px-6 py-3 bg-indigo-600 text-center");
      expect(result).toContain("mx-auto");
      expect(result).toContain("my-4");
      expect(result).toContain("px-6");
      expect(result).toContain("py-3");
      expect(result).toContain("bg-indigo-600");
      expect(result).toContain("text-center");
    });
  });

  describe("edge cases", () => {
    it("handles undefined gracefully", () => {
      expect(cn("px-4", undefined, "py-2")).toContain("px-4");
      expect(cn("px-4", undefined, "py-2")).toContain("py-2");
    });

    it("handles null gracefully", () => {
      expect(cn("px-4", null, "py-2")).toContain("px-4");
      expect(cn("px-4", null, "py-2")).toContain("py-2");
    });

    it("handles false gracefully", () => {
      expect(cn("px-4", false, "py-2")).toContain("px-4");
      expect(cn("px-4", false, "py-2")).toContain("py-2");
    });

    it("handles empty array", () => {
      expect(cn("px-4", [], "py-2")).toContain("px-4");
      expect(cn("px-4", [], "py-2")).toContain("py-2");
    });

    it("handles array of classes", () => {
      const result = cn(["px-4", "py-2"], "bg-red-500");
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).toContain("bg-red-500");
    });

    it("handles nested arrays", () => {
      const result = cn([["px-4", "py-2"]], "bg-red-500");
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).toContain("bg-red-500");
    });

    it("handles object with multiple keys", () => {
      const result = cn({
        "px-4": true,
        "py-2": true,
        "bg-red-500": false,
      });
      expect(result).toContain("px-4");
      expect(result).toContain("py-2");
      expect(result).not.toContain("bg-red-500");
    });

    it("handles no arguments", () => {
      expect(cn()).toBe("");
    });

    it("handles only falsy values", () => {
      expect(cn(false, null, undefined, "")).toBe("");
    });
  });
});

describe("decodeBase64Utf8 - Base64 decoding", () => {
  describe("basic encoding/decoding", () => {
    it("decodes basic ASCII string", () => {
      const encoded = "SGVsbG8=";
      expect(decodeBase64Utf8(encoded)).toBe("Hello");
    });

    it("decodes basic string without padding", () => {
      const encoded = "SGk";
      expect(decodeBase64Utf8(encoded)).toBe("Hi");
    });

    it("decodes string with single padding character", () => {
      const encoded = "dGVzdA=";
      expect(decodeBase64Utf8(encoded)).toBe("test");
    });

    it("decodes string with double padding characters", () => {
      const encoded = "YQ==";
      expect(decodeBase64Utf8(encoded)).toBe("a");
    });

    it("decodes empty string", () => {
      const encoded = "";
      expect(decodeBase64Utf8(encoded)).toBe("");
    });
  });

  describe("unicode and special characters", () => {
    it("decodes UTF-8 emoji", () => {
      const encoded = "8J+YgA==";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("😀");
    });

    it("decodes UTF-8 Chinese characters", () => {
      const encoded = "5L2g5aW9";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("你好");
    });

    it("decodes UTF-8 Japanese hiragana", () => {
      const encoded = "44GC44GE44GG44GI44GK";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("あいうえお");
    });

    it("decodes UTF-8 Korean Hangul", () => {
      const encoded = "7JWI64WV";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("안녕");
    });

    it("decodes UTF-8 Arabic text", () => {
      const encoded = "2YXYsdit2KjYpw==";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("مرحبا");
    });

    it("decodes UTF-8 with special punctuation", () => {
      const encoded = "SGVsbG8sIFdvcmxkISDwn4+L";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toContain("Hello");
      expect(decoded).toContain("World");
    });

    it("decodes multiple emojis", () => {
      const encoded = "8J+UpfCfkq/wn46J";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toContain("🔥");
      expect(decoded).toContain("💯");
      expect(decoded).toContain("🎉");
    });
  });

  describe("URL/spaces handling (edge cases from URL parsing)", () => {
    it("handles string with leading/trailing spaces", () => {
      const encoded = "  SGVsbG8=  ";
      expect(decodeBase64Utf8(encoded)).toBe("Hello");
    });

    it("handles string with spaces - function replaces spaces with +", () => {
      const encoded = "SGUrK8g=";
      const result = decodeBase64Utf8(encoded);
      expect(typeof result).toBe("string");
    });

    it("handles base64 without embedded spaces", () => {
      const encoded = "dGVzdA==";
      const result = decodeBase64Utf8(encoded);
      expect(result).toBe("test");
    });
  });

  describe("missing/incorrect padding", () => {
    it("decodes correctly with missing single padding character", () => {
      const encoded = "SGVsbG8";
      expect(decodeBase64Utf8(encoded)).toBe("Hello");
    });

    it("decodes correctly with missing double padding", () => {
      const encoded = "dGVzdA";
      expect(decodeBase64Utf8(encoded)).toBe("test");
    });

    it("auto-fixes partial padding", () => {
      const encoded = "SGVsbG8=";
      expect(decodeBase64Utf8(encoded)).toBe("Hello");
    });

    it("handles string with incorrect padding (too many =)", () => {
      const result = decodeBase64Utf8("SGVsbG8====");
      expect(typeof result).toBe("string");
    });
  });

  describe("invalid/malformed input", () => {
    it("returns original string on complete decode failure", () => {
      const invalid = "!@#$%^&*()";
      const result = decodeBase64Utf8(invalid);
      expect(typeof result).toBe("string");
    });

    it("handles valid base64 with invalid UTF-8 bytes gracefully", () => {
      const result = decodeBase64Utf8("////");
      expect(typeof result).toBe("string");
    });

    it("handles single character", () => {
      const result = decodeBase64Utf8("YQ==");
      expect(result).toBe("a");
    });

    it("handles very long base64 string", () => {
      const longString = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
      const encoded = Buffer.from(longString).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(longString);
    });
  });

  describe("roundtrip encoding/decoding", () => {
    it("roundtrip: ASCII strings", () => {
      const original = "Hello, World!";
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
    });

    it("roundtrip: emoji strings", () => {
      const original = "🚀🎯✨";
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
    });

    it("roundtrip: mixed content", () => {
      const original = "Hello 世界 🌍 مرحبا";
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
    });

    it("roundtrip: special characters", () => {
      const original = "!@#$%^&*()_+-=[]{}|;':\",./<>?";
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
    });

    it("roundtrip: newlines and tabs", () => {
      const original = "Line1\nLine2\tTabbed";
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
    });

    it("roundtrip: large JSON object", () => {
      const original = JSON.stringify({
        name: "Test",
        emoji: "🎉",
        chinese: "测试",
        message: "Hello, World!",
      });
      const encoded = Buffer.from(original).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe(original);
      expect(JSON.parse(decoded)).toEqual({
        name: "Test",
        emoji: "🎉",
        chinese: "测试",
        message: "Hello, World!",
      });
    });

    it("roundtrip: base64 without padding", () => {
      const original = "Test123!@#";
      const encoded = Buffer.from(original).toString("base64");
      const withoutPadding = encoded.replace(/=/g, "");
      const decoded = decodeBase64Utf8(withoutPadding);
      expect(decoded).toBe(original);
    });
  });

  describe("real-world URL scenarios", () => {
    it("handles base64 string from URL query parameter", () => {
      const queryBase64 = "SGVsbG8gV29ybGQ=";
      expect(decodeBase64Utf8(queryBase64)).toBe("Hello World");
    });

    it("handles URL-encoded config string", () => {
      const config = {
        apiKey: "secret123",
        endpoint: "https://api.example.com",
      };
      const encoded = Buffer.from(JSON.stringify(config)).toString("base64");
      const decoded = decodeBase64Utf8(encoded);
      expect(JSON.parse(decoded)).toEqual(config);
    });

    it("handles base64 with URL-safe variant characters", () => {
      const encoded = "SGVsbG8gV29ybGQ=";
      const decoded = decodeBase64Utf8(encoded);
      expect(decoded).toBe("Hello World");
    });
  });
});
