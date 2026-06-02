/**
 * 本地智能降级代理
 * 模拟 CC Switch 的降级功能：检测图片 → 自动切换多模态模型
 * 
 * 用法：node local-proxy.mjs YOUR_MIMO_API_KEY [port]
 * 默认端口：15722
 */

import http from 'http';
import { URL } from 'url';

const API_KEY = process.argv[2] || process.env.MIMO_API_KEY;
const PORT = parseInt(process.argv[3] || '15722', 10);

if (!API_KEY) {
  console.error('用法: node local-proxy.mjs YOUR_MIMO_API_KEY [port]');
  console.error('示例: node local-proxy.mjs tp-xxxxx 15722');
  process.exit(1);
}

// MiMo API 端点
const BASE_URL = API_KEY.startsWith('tp-')
  ? 'https://token-plan-cn.xiaomimimo.com/v1/chat/completions'
  : 'https://api.xiaomimimo.com/v1/chat/completions';

// 配置：主模型 → 降级模型映射
const FALLBACK_MAP = {
  'mimo-v2.5-pro': 'mimo-v2.5',
  // 可以在这里添加更多模型的降级配置
};

/**
 * 检测请求是否包含图片内容
 * 支持两种格式：
 * 1. OpenAI 格式: content[].type == "image_url"
 * 2. Anthropic 格式: content[].type == "image"
 */
function requestContainsImages(body) {
  if (!body.messages || !Array.isArray(body.messages)) {
    return false;
  }

  for (const message of body.messages) {
    if (!message.content || !Array.isArray(message.content)) {
      continue;
    }

    for (const part of message.content) {
      // OpenAI 格式
      if (part.type === 'image_url' && part.image_url?.url) {
        return true;
      }
      // Anthropic 格式 - 更严格的验证
      if (part.type === 'image') {
        // 检查 source 是否存在且有效
        if (part.source) {
          // base64 格式
          if (part.source.type === 'base64' && part.source.data) {
            return true;
          }
          // URL 格式
          if (part.source.type === 'url' && part.source.url) {
            return true;
          }
          // 兼容旧格式（直接检查 data 或 url）
          if (part.source.data || part.source.url) {
            return true;
          }
        }
      }
    }
  }

  return false;
}

/**
 * 执行模型降级
 */
function applyModelFallback(body) {
  const originalModel = body.model;
  const fallbackModel = FALLBACK_MAP[originalModel];

  if (!fallbackModel) {
    return { changed: false, originalModel, currentModel: originalModel };
  }

  if (requestContainsImages(body)) {
    body.model = fallbackModel;
    console.log(`[降级] 检测到图片内容，模型切换: ${originalModel} → ${fallbackModel}`);
    return { changed: true, originalModel, currentModel: fallbackModel };
  }

  return { changed: false, originalModel, currentModel: originalModel };
}

/**
 * 转发请求到 MiMo API（支持流式和非流式）
 */
async function forwardToMiMo(body, clientRes) {
  const isStream = body.stream === true;
  
  const response = await fetch(BASE_URL, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${API_KEY}`
    },
    body: JSON.stringify(body)
  });

  const status = response.status;
  
  // 设置响应头
  clientRes.writeHead(status, {
    'Content-Type': response.headers.get('content-type') || 'application/json',
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization',
  });

  if (isStream) {
    // 流式响应：直接 pipe 到客户端
    console.log(`  [响应] 流式转发，状态码: ${status}`);
    
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        
        const chunk = decoder.decode(value, { stream: true });
        clientRes.write(chunk);
        
        // 记录第一个 chunk
        if (chunk.length > 0) {
          const preview = chunk.substring(0, 100);
          console.log(`  [流] ${preview}${chunk.length > 100 ? '...' : ''}`);
        }
      }
    } catch (error) {
      console.error(`  [流错误] ${error.message}`);
    } finally {
      clientRes.end();
    }
    
    return { status, streamed: true };
  } else {
    // 非流式响应：读取完整响应
    const text = await response.text();
    
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = { raw: text };
    }

    console.log(`  [响应] 状态码: ${status}`);

    if (status === 200 && data.choices?.[0]?.message?.content) {
      const reply = data.choices[0].message.content;
      console.log(`  [回复] ${reply.substring(0, 100)}${reply.length > 100 ? '...' : ''}`);
    }

    clientRes.end(JSON.stringify(data));
    return { status, data, streamed: false };
  }
}

/**
 * 处理请求
 */
async function handleRequest(req, res) {
  // CORS 头
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');

  if (req.method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  if (req.method !== 'POST') {
    res.writeHead(405, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Method not allowed' }));
    return;
  }

  // 读取请求体
  let body = '';
  for await (const chunk of req) {
    body += chunk;
  }

  let parsed;
  try {
    parsed = JSON.parse(body);
  } catch {
    res.writeHead(400, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: 'Invalid JSON' }));
    return;
  }

  console.log(`\n[${new Date().toLocaleTimeString()}] 收到请求`);
  console.log(`  模型: ${parsed.model}`);
  console.log(`  消息数: ${parsed.messages?.length || 0}`);
  console.log(`  流式: ${parsed.stream ? '是' : '否'}`);

  // 应用降级逻辑
  const fallbackResult = applyModelFallback(parsed);

  if (fallbackResult.changed) {
    console.log(`  [降级] 已切换到多模态模型: ${fallbackResult.currentModel}`);
  } else if (FALLBACK_MAP[fallbackResult.originalModel]) {
    console.log(`  [正常] 使用原模型: ${fallbackResult.currentModel}（无图片内容）`);
  }

  // 转发请求
  try {
    await forwardToMiMo(parsed, res);
  } catch (error) {
    console.error(`  [错误] ${error.message}`);
    
    if (!res.headersSent) {
      res.writeHead(500, { 'Content-Type': 'application/json' });
    }
    res.end(JSON.stringify({ error: error.message }));
  }
}

// 创建服务器
const server = http.createServer(handleRequest);

server.listen(PORT, () => {
  console.log('='.repeat(60));
  console.log('本地智能降级代理已启动');
  console.log('='.repeat(60));
  console.log(`监听地址: http://127.0.0.1:${PORT}`);
  console.log(`MiMo API: ${BASE_URL}`);
  console.log(`API Key: ${API_KEY.substring(0, 10)}...`);
  console.log('\n降级配置:');
  for (const [from, to] of Object.entries(FALLBACK_MAP)) {
    console.log(`  ${from} → ${to}（当请求包含图片时）`);
  }
  console.log('\n功能:');
  console.log('  ✅ 支持流式响应 (stream: true)');
  console.log('  ✅ 支持非流式响应');
  console.log('  ✅ 图片检测自动降级');
  console.log('\n使用方法:');
  console.log(`  在 Codex 中配置代理地址为: http://127.0.0.1:${PORT}`);
  console.log('='.repeat(60));
});

// 优雅退出
process.on('SIGINT', () => {
  console.log('\n正在关闭代理...');
  server.close(() => {
    console.log('代理已关闭');
    process.exit(0);
  });
});