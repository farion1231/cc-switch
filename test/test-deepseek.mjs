/**
 * DeepSeek API 测试：验证 v4-pro 不支持图片 vs v4 支持图片
 */

const API_KEY = process.argv[2];
if (!API_KEY) { console.error("用法: node test-deepseek.mjs YOUR_KEY"); process.exit(1); }

const BASE_URL = "https://api.deepseek.com/v1/chat/completions";
const TINY_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";

async function testModel(modelName, withImage) {
  const messages = withImage
    ? [{
        role: "user",
        content: [
          { type: "text", text: "这张图片是什么颜色？请用中文简短回答。" },
          { type: "image_url", image_url: { url: `data:image/png;base64,${TINY_PNG}` } }
        ]
      }]
    : [{ role: "user", content: "你好，请回复'OK'" }];

  console.log(`\n--- ${modelName} | ${withImage ? "含图片" : "纯文本"} ---`);
  try {
    const resp = await fetch(BASE_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json", "Authorization": `Bearer ${API_KEY}` },
      body: JSON.stringify({ model: modelName, messages, max_tokens: 50, stream: false })
    });
    const text = await resp.text();
    if (resp.status === 200) {
      const data = JSON.parse(text);
      const reply = data.choices?.[0]?.message?.content || "(empty)";
      console.log(`  ✅ 成功 (${resp.status}): ${reply.substring(0, 100)}`);
      return { success: true };
    } else {
      console.log(`  ❌ 失败 (${resp.status}): ${text.substring(0, 200)}`);
      return { success: false, status: resp.status };
    }
  } catch (err) {
    console.log(`  ❌ 错误: ${err.message}`);
    return { success: false };
  }
}

async function main() {
  console.log("=== DeepSeek API 图片支持验证 ===\n");

  // 测试各模型
  const r1 = await testModel("deepseek-chat", false);        // 基线：纯文本
  const r2 = await testModel("deepseek-chat", true);         // deepseek-chat 含图片
  const r3 = await testModel("deepseek-reasoner", false);    // reasoner 纯文本
  const r4 = await testModel("deepseek-reasoner", true);     // reasoner 含图片

  console.log("\n=== 结果汇总 ===");
  console.log(`  deepseek-chat     纯文本: ${r1.success ? "✅" : "❌"}`);
  console.log(`  deepseek-chat     含图片: ${r2.success ? "✅" : "❌"}`);
  console.log(`  deepseek-reasoner 纯文本: ${r3.success ? "✅" : "❌"}`);
  console.log(`  deepseek-reasoner 含图片: ${r4.success ? "✅" : "❌"}`);

  if (r2.success && !r4.success) {
    console.log("\n  ✅ deepseek-chat 支持图片，deepseek-reasoner 不支持 → 降级算法有效");
  } else if (!r2.success && !r4.success) {
    console.log("\n  ⚠️ DeepSeek 所有模型都不支持 OpenAI 格式的图片输入");
    console.log("  → 需要检查 DeepSeek 的图片 API 格式");
  }
}
main().catch(console.error);