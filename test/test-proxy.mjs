/**
 * 通过 CC Switch 代理测试 MiMo 图片请求
 * 测试 Failover 是否能自动处理
 */

const PROXY_URL = "http://127.0.0.1:15721/v1/messages";
const TINY_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";

async function testProxy(label, body) {
  console.log(`\n--- ${label} ---`);
  try {
    const resp = await fetch(PROXY_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-api-key": "test-key",
        "anthropic-version": "2023-06-01"
      },
      body: JSON.stringify(body)
    });
    const text = await resp.text();
    if (resp.ok) {
      console.log(`  ✅ 成功 (${resp.status}): ${text.substring(0, 150)}`);
    } else {
      console.log(`  ❌ 失败 (${resp.status}): ${text.substring(0, 300)}`);
    }
    return resp.ok;
  } catch (err) {
    console.log(`  ❌ 错误: ${err.message}`);
    return false;
  }
}

async function main() {
  console.log("=== CC Switch 代理 Failover 测试 ===\n");

  // 测试 1: Claude 格式纯文本
  await testProxy("Claude 格式 纯文本", {
    model: "claude-sonnet-4-5",
    max_tokens: 20,
    messages: [{ role: "user", content: "请回复OK" }]
  });

  // 测试 2: Claude 格式含图片
  await testProxy("Claude 格式 含图片", {
    model: "claude-sonnet-4-5",
    max_tokens: 50,
    messages: [{
      role: "user",
      content: [
        { type: "text", text: "这张图片是什么？" },
        { type: "image", source: { type: "base64", media_type: "image/png", data: TINY_PNG } }
      ]
    }]
  });

  // 测试 3: Responses API 格式纯文本 (Codex 使用的格式)
  await testProxy("Responses API 纯文本", {
    model: "gpt-5.5",
    input: [{ role: "user", content: [{ type: "input_text", text: "请回复OK" }] }]
  });

  // 测试 4: Responses API 格式含图片
  await testProxy("Responses API 含图片", {
    model: "gpt-5.5",
    input: [{
      type: "message",
      role: "user",
      content: [
        { type: "input_text", text: "这张图片是什么？" },
        { type: "input_image", image_url: `data:image/png;base64,${TINY_PNG}` }
      ]
    }]
  });
}

main().catch(console.error);