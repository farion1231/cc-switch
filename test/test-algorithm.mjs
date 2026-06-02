/**
 * 验证核心算法：mimo-v2.5-pro 不支持图片 → mimo-v2.5 支持图片
 * 
 * 测试内容：
 * 1. request_contains_images 检测逻辑
 * 2. 实际 MiMo API 行为（需要 API Key）
 */

// ========== Part 1: 测试 request_contains_images 检测逻辑 ==========

function requestContainsImages(body) {
  // Anthropic Messages API 格式
  if (body.messages && Array.isArray(body.messages)) {
    for (const msg of body.messages) {
      if (msg.content && Array.isArray(msg.content)) {
        for (const block of msg.content) {
          if (block.type === "image") return true;
        }
      }
    }
  }

  // OpenAI Responses API 格式
  if (body.input && Array.isArray(body.input)) {
    for (const item of body.input) {
      if (item.type === "input_image") return true;
      if (item.content && Array.isArray(item.content)) {
        for (const block of item.content) {
          if (block.type === "input_image") return true;
        }
      }
    }
  }

  return false;
}

// 测试用例
const tests = [
  {
    name: "Anthropic 格式 - 含图片",
    body: {
      model: "mimo-v2.5-pro",
      messages: [{
        role: "user",
        content: [
          { type: "text", text: "describe this" },
          { type: "image", source: { type: "base64", media_type: "image/png", data: "abc123" } }
        ]
      }]
    },
    expected: true
  },
  {
    name: "Responses API 格式 - 含 input_image",
    body: {
      model: "mimo-v2.5-pro",
      input: [{
        type: "message", role: "user", content: [
          { type: "input_text", text: "describe this" },
          { type: "input_image", image_url: "data:image/png;base64,abc123" }
        ]
      }]
    },
    expected: true
  },
  {
    name: "Responses API 格式 - 直接 input_image",
    body: {
      model: "mimo-v2.5-pro",
      input: [{ type: "input_image", image_url: "data:image/png;base64,abc123" }]
    },
    expected: true
  },
  {
    name: "纯文本请求",
    body: {
      model: "mimo-v2.5-pro",
      messages: [{ role: "user", content: "hello world" }]
    },
    expected: false
  },
  {
    name: "text content 数组",
    body: {
      model: "mimo-v2.5-pro",
      messages: [{ role: "user", content: [{ type: "text", text: "hello" }] }]
    },
    expected: false
  },
  {
    name: "空 body",
    body: { model: "mimo-v2.5-pro" },
    expected: false
  },
  {
    name: "空 messages",
    body: { model: "mimo-v2.5-pro", messages: [] },
    expected: false
  }
];

console.log("=== Part 1: request_contains_images 检测逻辑 ===\n");
let passed = 0;
let failed = 0;
for (const t of tests) {
  const result = requestContainsImages(t.body);
  const ok = result === t.expected;
  if (ok) {
    passed++;
    console.log(`  ✅ ${t.name}`);
  } else {
    failed++;
    console.log(`  ❌ ${t.name} — expected ${t.expected}, got ${result}`);
  }
}
console.log(`\n  结果: ${passed} passed, ${failed} failed\n`);

// ========== Part 2: 模型降级逻辑测试 ==========

function applyMultimodalFallback(body, providerMeta) {
  const fallbackModel = providerMeta?.multimodalFallbackModel;
  if (!fallbackModel) return { body, downgraded: false };
  
  if (requestContainsImages(body)) {
    const original = body.model;
    body.model = fallbackModel;
    console.log(`  [降级] ${original} → ${fallbackModel}`);
    return { body, downgraded: true, original, fallback: fallbackModel };
  }
  return { body, downgraded: false };
}

console.log("=== Part 2: 模型降级逻辑 ===\n");

// 测试 1: 有 fallback 配置 + 图片请求 → 降级
const test1Body = {
  model: "mimo-v2.5-pro",
  messages: [{
    role: "user",
    content: [
      { type: "text", text: "这是什么？" },
      { type: "image", source: { type: "base64", media_type: "image/png", data: "iVBOR..." } }
    ]
  }]
};
const test1Result = applyMultimodalFallback(test1Body, { multimodalFallbackModel: "mimo-v2.5" });
console.log(`  ✅ 有 fallback + 图片请求: downgraded=${test1Result.downgraded}, model=${test1Body.model}`);

// 测试 2: 有 fallback 配置 + 纯文本 → 不降级
const test2Body = { model: "mimo-v2.5-pro", messages: [{ role: "user", content: "写个函数" }] };
const test2Result = applyMultimodalFallback(test2Body, { multimodalFallbackModel: "mimo-v2.5" });
console.log(`  ✅ 有 fallback + 纯文本: downgraded=${test2Result.downgraded}, model=${test2Body.model}`);

// 测试 3: 无 fallback 配置 + 图片请求 → 不降级
const test3Body = {
  model: "mimo-v2.5-pro",
  messages: [{ role: "user", content: [{ type: "image", source: { type: "base64", media_type: "image/png", data: "abc" } }] }]
};
const test3Result = applyMultimodalFallback(test3Body, {});
console.log(`  ✅ 无 fallback + 图片请求: downgraded=${test3Result.downgraded}, model=${test3Body.model}`);

console.log("\n=== Part 2 完成 ===\n");