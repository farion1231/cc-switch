/**
 * 测速工具 - 跨平台兼容版本
 */

export interface SpeedTestResult {
  endpoint: string;
  latency: number; // 毫秒
  success: boolean;
  error?: string;
}

/**
 * 测试单个节点的延迟（前端备用方法）
 */
export async function testEndpointSpeed(
  endpoint: string,
  timeout: number = 8000
): Promise<SpeedTestResult> {
  const startTime = performance.now();
  
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeout);
    
    // 构造测试 URL，优先使用 favicon 或其他轻量资源
    let testUrl = endpoint;
    if (!testUrl.includes('/favicon') && !testUrl.includes('/ping') && !testUrl.includes('/health')) {
      // 尝试添加常见的健康检查路径
      testUrl = `${endpoint.replace(/\/$/, '')}/favicon.ico`;
    }
    
    // 发送请求测试连通性
    const response = await fetch(testUrl, {
      method: "GET",
      signal: controller.signal,
      mode: "no-cors", // 避免 CORS 问题
      cache: "no-cache",
      redirect: "follow",
      headers: {
        "User-Agent": "CC-Switch-SpeedTest/1.0",
      },
    });
    
    clearTimeout(timeoutId);
    const endTime = performance.now();
    const latency = Math.round(endTime - startTime);
    
    return {
      endpoint,
      latency,
      success: latency < 10000, // 10秒内认为成功
    };
  } catch (error) {
    const endTime = performance.now();
    const latency = Math.round(endTime - startTime);
    
    // 对于 no-cors 模式，网络错误可能仍然表示连通
    const isNetworkError = error instanceof Error && 
      (error.name === 'TypeError' || error.message.includes('Failed to fetch'));
    
    return {
      endpoint,
      latency: Math.min(latency, 10000), // 限制最大延迟显示
      success: isNetworkError && latency < 8000, // 网络错误但延迟合理可能表示连通
      error: error instanceof Error ? error.message : "未知错误",
    };
  }
}

/**
 * 测试多个节点并返回按延迟排序的结果
 */
export async function testMultipleEndpoints(
  endpoints: string[],
  concurrency: number = 5 // 限制并发数以避免过载
): Promise<SpeedTestResult[]> {
  if (endpoints.length === 0) {
    return [];
  }
  
  const results: SpeedTestResult[] = [];
  
  // 分批并发测试以避免过载
  for (let i = 0; i < endpoints.length; i += concurrency) {
    const batch = endpoints.slice(i, i + concurrency);
    const batchPromises = batch.map((endpoint) => testEndpointSpeed(endpoint));
    
    try {
      const batchResults = await Promise.allSettled(batchPromises);
      
      batchResults.forEach((result, index) => {
        if (result.status === 'fulfilled') {
          results.push(result.value);
        } else {
          // 处理 Promise 失败的情况
          results.push({
            endpoint: batch[index],
            latency: 10000,
            success: false,
            error: result.reason?.message || "测试失败",
          });
        }
      });
    } catch (error) {
      console.error("批量测试失败:", error);
      // 为失败的批次添加错误结果
      batch.forEach((endpoint) => {
        results.push({
          endpoint,
          latency: 10000,
          success: false,
          error: "批量测试异常",
        });
      });
    }
  }
  
  // 按延迟排序，成功的节点优先
  return results.sort((a, b) => {
    if (a.success && !b.success) return -1;
    if (!a.success && b.success) return 1;
    return a.latency - b.latency;
  });
}

/**
 * 获取最快的可用节点
 */
export async function getFastestEndpoint(
  endpoints: string[]
): Promise<string | null> {
  const results = await testMultipleEndpoints(endpoints);
  const fastest = results.find((r) => r.success);
  return fastest ? fastest.endpoint : null;
}

/**
 * 检查网络连接状态
 */
export function isOnline(): boolean {
  return typeof navigator !== 'undefined' ? navigator.onLine : true;
}

/**
 * 格式化延迟显示
 */
export function formatLatency(latency: number): string {
  if (latency >= 10000) {
    return "超时";
  }
  if (latency >= 1000) {
    return `${(latency / 1000).toFixed(1)}s`;
  }
  return `${latency}ms`;
}