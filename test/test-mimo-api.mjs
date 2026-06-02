/**
 * 真实 MiMo API 测试：验证 mimo-v2.5-pro 拒绝图片 vs mimo-v2.5 接受图片
 */
const API_KEY = process.argv[2] || process.env.MIMO_API_KEY;
if (!API_KEY) {
  console.error("用法: node test-mimo-api.mjs YOUR_MIMO_API_KEY");
  process.exit(1);
}

// tp- 前缀是 Token Plan，使用国内端点
const BASE_URL = API_KEY.startsWith("tp-")
  ? "https://token-plan-cn.xiaomimimo.com/v1/chat/completions"
  : "https://api.xiaomimimo.com/v1/chat/completions";

// 一个小的 1x1 红色 PNG 图片 (base64)
const TINY_PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";

async function testModel(modelName, withImage) {
  const messages = withImage
    ? [{
        role: "user",
        content: [
          { type: "text", text: "这张图片是什么颜色？请用中文回答。" },
          { type: "image_url", image_url: { url: `data:image/png;base64,${TINY_PNG}` } }
        ]
      }]
    : [{
        role: "user",
        content: "你好，请用中文回复'OK'"
      }];

  const body = { model: modelName, messages, max_tokens: 50, stream: false };

  console.log(`\n--- 测试: ${modelName} | ${withImage ? "含图片" : "纯文本"} ---`);
  
  try {
    const resp = await fetch(BASE_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Authorization": `Bearer ${API_KEY}`
      },
      body: JSON.stringify(body)
    });

    const status = resp.status;
    const text = await resp.text();
    
    if (status === 200) {
      const data = JSON.parse(text);
      const reply = data.choices?.[0]?.message?.content || "(empty)";
      console.log(`  ✅ 成功 (${status}): ${reply.substring(0, 100)}`);
      return { success: true, status, reply };
    } else {
      console.log(`  ❌ 失败 (${status}): ${text.substring(0, 300)}`);
      return { success: false, status, error: text.substring(0, 300) };
    }
  } catch (err) {
    console.log(`  ❌ 网络错误: ${err.message}`);
    return { success: false, error: err.message };
  }
}

async function main() {
  console.log("=== MiMo API 图片支持验证测试 ===");
  console.log(`API: ${BASE_URL}`);
  console.log(`API Key: ${API_KEY.substring(0, 8)}...`);

  // 测试 1: mimo-v2.5-pro 纯文本 (应该成功)
  const r1 = await testModel("mimo-v2.5-pro", false);

  // 测试 2: mimo-v2.5-pro 含图片 (预期失败 - 400)
  const r2 = await testModel("mimo-v2.5-pro", true);

  // 测试 3: mimo-v2.5 含图片 (预期成功 - 多模态)
  const r3 = await testModel("mimo-v2.5", true);

  // 测试 4: mimo-v2.5 纯文本 (应该也成功)
  const r4 = await testModel("mimo-v2.5", false);

  console.log("\n=== 测试结果汇总 ===");
  console.log(`  mimo-v2.5-pro 纯文本: ${r1.success ? "✅ 成功" : "❌ 失败"}`);
  console.log(`  mimo-v2.5-pro 含图片: ${r2.success ? "✅ 成功" : "❌ 失败 (预期)"}`);
  console.log(`  mimo-v2.5   含图片: ${r3.success ? "✅ 成功" : "❌ 失败"}`);
  console.log(`  mimo-v2.5   纯文本: ${r4.success ? "✅ 成功" : "❌ 失败"}`);

  console.log("\n=== 算法验证结论 ===");
  if (!r2.success && r3.success) {
    console.log("  ✅ 验证通过！mimo-v2.5-pro 不支持图片，mimo-v2.5 支持图片");
    console.log("  → 自动降级算法有效：检测到图片时从 mimo-v2.5-pro 切换到 mimo-v2.5");
  } else if (r2.success && r3.success) {
    console.log("  ⚠️ 两个模型都支持图片！降级逻辑仍然安全（不会误触发）");
  } else if (!r2.success && !r3.success) {
    console.log("  ⚠️ 两个模型都不支持图片请求，需要确认正确的多模态模型名称");
    console.log("  → 可能 mimo-v2.5 的图片格式与 OpenAI Chat Completions 不同");
  }
}

main().catch(console.error);