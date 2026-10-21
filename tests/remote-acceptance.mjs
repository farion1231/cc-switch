#!/usr/bin/env node
/**
 * 远程管理 —— HTTP 端到端验收脚本。
 *
 * 它回答的问题是「一个**真的 HTTP 客户端**从外面打进来会看到什么」：
 * 根页面开不开、受保护的路由认不认凭据、白名单里的 Origin 放不放行、
 * Host 闸门拦的是不是只有匿名请求。这些正是 crate 内部那批 E2E 用例
 * （`src-tauri/src/remote/e2e.rs`）看不见的一层 —— 它们自己造请求、自己
 * 调私有 API，验的是「判定表那一格」。
 *
 * 用法：
 *
 * ```text
 * # 1) 另一个终端起夹具（它会一直活着）：
 * cd src-tauri && cargo test --lib remote_e2e_fixture -- --ignored --nocapture
 * # 2) 本终端跑验收：
 * node tests/remote-acceptance.mjs
 * ```
 *
 * 自检（不碰真服务，进程内起一个 mock server 跑同一套断言引擎）：
 *
 * ```text
 * node tests/remote-acceptance.mjs --self-test
 * ```
 *
 * 对着**真机**跑（浏览器级验收那一步，服务跑在 `pnpm tauri dev` 起的那份上，
 * 夹具文件并不存在）：把三项用环境变量喂进来 ——
 *
 * ```text
 * CC_SWITCH_E2E_BASE_URL=http://127.0.0.1:4000 \
 * CC_SWITCH_E2E_SESSION_COOKIE=<配对拿到的会话 cookie 值> \
 * CC_SWITCH_E2E_BEARER=<配对拿到的 token> \
 * node tests/remote-acceptance.mjs
 * ```
 *
 * 浏览器级验收（操作者执行，本脚本不碰浏览器）：
 *
 * 1. `pnpm tauri dev` 起应用，在设置页启用远程管理；另开终端跑本脚本。
 * 2. Playwright 打开 `http://127.0.0.1:4000`，模拟用户输入配对码并点「配对」。
 * 3. 断言：供应商卡片数 ≥1；点非当前卡片后出现 "Switched" toast；截图
 *    `/tmp/cc-switch-remote-e2e/step3-switch.png`；控制台无 error。
 *
 * 依赖：只用 Node 内置模块（`node:http` / `node:assert` / `node:fs`），不引新依赖。
 *
 * 退出码：0 = 六条断言全过；1 = 有断言没过，或夹具读不到。
 */

import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import process from 'node:process';

/* ==========================================================================
 * 契约
 * ======================================================================== */

/**
 * 夹具文件路径。
 *
 * 与 `src-tauri/src/remote/e2e.rs` 里的 `REMOTE_E2E_FIXTURE_PATH` **逐字相同**
 * —— 两边各写一份而没有共同来源，路径一旦分叉，操作者看到的现象是「夹具没写
 * 出来」，而真实原因是两个文件对不上。
 */
const FIXTURE_PATH = '/tmp/cc-switch-remote-e2e/fixture.json';

/** 夹具缺失时，操作者唯一能做的事就是照这条命令起夹具。 */
const FIXTURE_HINT =
  'cd src-tauri && cargo test --lib remote_e2e_fixture -- --ignored --nocapture';

/** 单个请求的超时上限。 */
const REQUEST_TIMEOUT_MS = 5000;

/**
 * 「另一台设备的 MagicDNS 名」。
 *
 * 故意**不**是本机自己的 MagicDNS 名：本机自己的名字在 Host 白名单里
 * （`allowed_hosts` 收了 `service_names()`），拿它当 Host 时下面那条断言恒真，
 * 什么也验不到。用一台不在本机名单里的设备名，这条断言才真的在问：
 * 「带凭据的请求会不会被 Host 闸门拦下来」。
 */
const PEER_MAGICDNS_HOST = 'cc-switch-e2e-peer.tail1234.ts.net';

/** 匿名请求用的坏 Host（DNS rebinding 的服务端那一半）。 */
const EVIL_HOST = 'evil.example';

/**
 * 代理转发信号头。
 *
 * 带上它，这一发请求就不再是「本机内嵌页面」那一格（loopback + 无代理信号在判定
 * 表里是**放行**的 —— 那是同一台机器上的内嵌 Web UI），而是「本机代理在替远端
 * 客户端转发」那一格，后者要求凭据。
 *
 * 验收要问的正是后者：「凭据到底认不认」。不带这个头的话，匿名请求在本机直连
 * 通道上本来就被放行，于是「带 cookie / 带 bearer 才有 200」这件事根本看不出来
 * —— 断言会恒真，而恒真的断言什么也没验。
 */
const PROXY_SIGNAL_HEADERS = { 'X-Forwarded-For': '203.0.113.7' };

/* ==========================================================================
 * HTTP
 * ======================================================================== */

/**
 * 发一个 HTTP 请求，5s 超时。
 *
 * 超时按「整个请求」计：连不上、服务不响应、响应不结束，三种都算超时 ——
 * 验收脚本最忌讳的是「卡住」被当成「慢」，所以宁可明确失败。
 *
 * @param {string} baseUrl
 * @param {{method?: string, path?: string, headers?: Record<string,string>, body?: string|null}} options
 * @returns {Promise<{status: number, headers: Record<string, any>, body: string}>}
 */
function httpRequest(baseUrl, options = {}) {
  const { method = 'GET', path = '/', headers = {}, body = null } = options;
  return new Promise((resolve, reject) => {
    const url = new URL(path, baseUrl);
    const request = http.request(
      {
        method,
        hostname: url.hostname,
        port: url.port === '' ? 80 : Number(url.port),
        path: `${url.pathname}${url.search}`,
        headers,
        timeout: REQUEST_TIMEOUT_MS,
      },
      (response) => {
        const chunks = [];
        response.on('data', (chunk) => chunks.push(chunk));
        response.on('end', () => {
          resolve({
            status: response.statusCode,
            headers: response.headers,
            body: Buffer.concat(chunks).toString('utf8'),
          });
        });
      },
    );
    request.on('timeout', () => {
      request.destroy(new Error(`请求超时（> ${REQUEST_TIMEOUT_MS}ms）：${method} ${path}`));
    });
    request.on('error', reject);
    if (body !== null) {
      request.write(body);
    }
    request.end();
  });
}

/* ==========================================================================
 * 断言引擎
 * ======================================================================== */

/**
 * 一条断言的结果。
 *
 * `passLine` / `failLine` 是给人看的那一行；`ok` 是机器看的那一位。失败行必须
 * 带**实际状态码** —— 「期望 401，实际 200」与「期望 401，实际 503」是两种
 * 完全不同的故障，混成一句「断言失败」等于把诊断成本转嫁给读报告的人。
 *
 * `note` 是可选的补充说明：有一格的状态码对了、但事情本身没成（见切换那一格
 * 在 headless 夹具里的情形），不写出来就会让人把「PASS」读成「切换成功了」。
 *
 * @typedef {{label: string, ok: boolean, passLine: string, failLine: string, note?: string}} Check
 */

/**
 * 期望某个确切状态码。
 *
 * @param {string} label
 * @param {number} expected
 * @param {{status: number}} response
 * @returns {Check}
 */
function expectStatus(label, expected, response) {
  const actual = response.status;
  const ok = actual === expected;
  return {
    label,
    ok,
    passLine: `${label} => ${expected}`,
    failLine: `${label} => expected ${expected}, actual ${actual}`,
  };
}

/**
 * 期望「没有被 Host 闸门拦下」。
 *
 * 判定是「不是 403，**也不是 401**」：403 说明 Host 闸门把带凭据的请求也拦了
 * （那正是 Host 闸门只兜匿名那一格这条规则要防的回归），401 说明凭据本身没被
 * 认 —— 两者都不是「放行」。
 *
 * @param {string} label
 * @param {{status: number}} response
 * @returns {Check}
 */
function expectNotForbidden(label, response) {
  const actual = response.status;
  const ok = actual !== 403 && actual !== 401;
  return {
    label,
    ok,
    passLine: `${label} => not 403 (actual ${actual})`,
    failLine: `${label} => expected not 403, actual ${actual}`,
  };
}

/**
 * 请求本身失败（连不上、超时）时的那一条。
 *
 * 单独一种形状而不是「当作状态码 0」：连接层失败与服务端答错是两回事，
 * 混在一起报出来会把「服务没起」说成「服务答错了」。
 *
 * @param {string} label
 * @param {Error} error
 * @returns {Check}
 */
function expectFailure(label, error) {
  return {
    label,
    ok: false,
    passLine: `${label} => 200`,
    failLine: `${label} => 请求失败：${error.message}`,
  };
}

/**
 * 取一个可切换的供应商 id。
 *
 * 取不到时（headless 夹具里没有供应商）退回一个占位 id：这一格要验的是
 * 「带白名单 Origin 的请求有没有被 Origin 闸门否决」，不是切换业务本身 ——
 * 切换业务由 crate 内部那批 E2E 用例负责（那边能断言到 SSE 帧里的
 * provider_id）。
 *
 * @param {string} baseUrl
 * @param {string} bearer
 * @returns {Promise<string>}
 */
async function pickProviderId(baseUrl, bearer) {
  try {
    const response = await httpRequest(baseUrl, {
      path: '/api/providers',
      headers: { Authorization: `Bearer ${bearer}` },
    });
    const parsed = JSON.parse(response.body);
    const providers = Array.isArray(parsed) ? parsed : parsed?.providers;
    if (Array.isArray(providers) && providers.length > 0) {
      const id = providers[0]?.id;
      if (typeof id === 'string' && id !== '') {
        return id;
      }
    }
  } catch {
    // 列表读不到就退回占位 id：下面那一格验的是 Origin 闸门，不是列表。
  }
  return 'acceptance-switch-target';
}

/**
 * 六条断言 —— 与 `src-tauri/src/remote/e2e.rs` 里那套判定表一一对应。
 *
 * @param {{baseUrl: string, sessionCookie: string, bearer: string, port: string}} target
 * @returns {Promise<Check[]>}
 */
async function runChecks(target) {
  const { baseUrl, sessionCookie, bearer, port } = target;
  const origin = `http://localhost:${port}`;

  /** 跑一条断言，把连接层失败也收成一条「失败的断言」而不是异常。 */
  const check = async (label, run) => {
    try {
      const response = await run();
      return response;
    } catch (error) {
      return expectFailure(label, error);
    }
  };

  const results = [];

  // 1. 根页面是公开的：任何人都能打开内嵌 Web UI。
  results.push(
    await check('anonymous GET /', async () => {
      const response = await httpRequest(baseUrl, { path: '/' });
      return expectStatus('anonymous GET /', 200, response);
    }),
  );

  // 2. 受保护的路由对匿名请求关门。
  //
  //    这一发走「本机代理在转发」那一格（判定表第三列，见 [`PROXY_SIGNAL_HEADERS`]
  //    的说明）：本机直连那一格对本机内嵌页面是放行的，匿名请求答 200 —— 那样
  //    这一格就恒真了。换成代理通道之后，匿名 → 401。
  results.push(
    await check('anonymous GET /api/providers', async () => {
      const response = await httpRequest(baseUrl, {
        path: '/api/providers',
        headers: { ...PROXY_SIGNAL_HEADERS },
      });
      return expectStatus('anonymous GET /api/providers', 401, response);
    }),
  );

  // 3. 会话 cookie 那条通道：配对交付的 cookie 能打开受保护的路由。
  //
  //    同样走代理通道：少了这一层，「cookie 有效」与「本机内嵌页面免凭据」就分
  //    不出来了 —— 两种情形都是 200。
  results.push(
    await check('cookie GET /api/providers', async () => {
      const response = await httpRequest(baseUrl, {
        path: '/api/providers',
        headers: {
          ...PROXY_SIGNAL_HEADERS,
          Cookie: `cc_switch_session=${sessionCookie}`,
        },
      });
      return expectStatus('cookie GET /api/providers', 200, response);
    }),
  );

  // 4. 白名单里的 Origin 不被否决。
  results.push(
    await check(`POST /api/switch Origin ${origin}`, async () => {
      const providerId = await pickProviderId(baseUrl, bearer);
      const response = await httpRequest(baseUrl, {
        method: 'POST',
        path: '/api/switch',
        headers: {
          Authorization: `Bearer ${bearer}`,
          Origin: origin,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ provider_id: providerId }),
      });
      const result = expectStatus(`POST /api/switch Origin ${origin}`, 200, response);
      // headless 夹具里没有供应商，切换业务会答「200 + success:false」（错误也走
      // 200 那一格 —— 那是 handler 的既有形状）。这一格要验的是「带白名单 Origin
      // 的请求没被 Origin 闸门否决」，不是切换本身；不把这一点写出来，读的人会
      // 把 PASS 读成「切换成功了」。
      try {
        const parsed = JSON.parse(response.body);
        if (parsed && parsed.success === false) {
          result.note = `切换业务本身没成功（${parsed.error ?? '原因未给出'}）：这一格验的是白名单 Origin 没被 Origin 闸门否决`;
        }
      } catch {
        // 响应不是 JSON：状态码那一格已经验完了，不必多说。
      }
      return result;
    }),
  );

  // 5. 带凭据的请求不受 Host 闸门拦 —— DNS rebinding 的服务端那一半只兜匿名。
  //
  //    对照组是第 6 格（同一个 Host，匿名 → 403）：两格合起来才说得清「Host
  //    闸门拦的是匿名请求，不是带凭据的请求」。少了 bearer，这一格会变成 403。
  results.push(
    await check(`bearer + Host ${PEER_MAGICDNS_HOST}`, async () => {
      const response = await httpRequest(baseUrl, {
        path: '/api/providers',
        headers: {
          ...PROXY_SIGNAL_HEADERS,
          Authorization: `Bearer ${bearer}`,
          Host: PEER_MAGICDNS_HOST,
        },
      });
      return expectNotForbidden(`bearer + Host ${PEER_MAGICDNS_HOST}`, response);
    }),
  );

  // 6. 匿名请求的 Host 不在名单里 → 403（这一格是 Host 闸门本身）。
  results.push(
    await check(`anonymous Host ${EVIL_HOST}`, async () => {
      const response = await httpRequest(baseUrl, {
        path: '/api/providers',
        headers: { Host: EVIL_HOST },
      });
      return expectStatus(`anonymous Host ${EVIL_HOST}`, 403, response);
    }),
  );

  return results;
}

/* ==========================================================================
 * mock server（自检用）
 * ======================================================================== */

/**
 * 进程内 mock server：把判定表按最简形式重演一遍。
 *
 * 它不是一张随手编的状态码表 —— 每一格都**看请求实际带的头**再决定答什么。
 * 这一点是自检的全部价值：断言引擎若忘了带 `Host`、忘了带 cookie、忘了带
 * `X-Forwarded-For`，mock 给出的答案就不一样，引擎就会判 FAIL —— 于是
 * 「断言引擎恒真」这件事在自检里就暴露了。
 *
 * @param {{broken?: string|null}} options `broken` 指定哪一格故意答错（负控用）
 */
function startMockServer(options = {}) {
  const { broken = null } = options;
  const bearer = 'mock-bearer-token';
  const sessionCookie = 'mock-session-cookie';

  const server = http.createServer((request, response) => {
    const host = request.headers.host ?? '';
    const hostName = host.split(':')[0];
    const allowedHosts = new Set(['127.0.0.1', 'localhost']);
    const hasBearer = request.headers.authorization === `Bearer ${bearer}`;
    const hasCookie = (request.headers.cookie ?? '').includes(
      `cc_switch_session=${sessionCookie}`,
    );
    const hasProxySignal = ['x-forwarded-for', 'forwarded', 'tailscale-user-login'].some(
      (name) => name in request.headers,
    );
    const origin = request.headers.origin;

    const send = (status, payload) => {
      const body = JSON.stringify(payload);
      response.writeHead(status, {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(body),
      });
      response.end(body);
    };

    if (request.method === 'GET' && request.url === '/') {
      send(200, { page: 'mock' });
      return;
    }

    if (request.method === 'GET' && request.url === '/api/providers') {
      if (hasBearer || hasCookie) {
        send(200, { providers: [] });
        return;
      }
      // 匿名：先过 Host 闸门，再看通道。
      if (!allowedHosts.has(hostName)) {
        send(403, { error: 'forbidden_host' });
        return;
      }
      if (hasProxySignal) {
        send(broken === 'anonymousProviders' ? 500 : 401, { error: 'unauthorized' });
        return;
      }
      send(200, { providers: [] });
      return;
    }

    if (request.method === 'POST' && request.url === '/api/switch') {
      const allowedOrigin = `http://localhost:${portOf(request)}`;
      if (origin !== undefined && origin !== allowedOrigin) {
        send(403, { error: 'forbidden_origin' });
        return;
      }
      send(200, { success: true });
      return;
    }

    send(404, { error: 'not_found' });
  });

  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address !== null ? address.port : 0;
      resolve({
        port,
        bearer,
        sessionCookie,
        target: {
          baseUrl: `http://127.0.0.1:${port}`,
          sessionCookie,
          bearer,
          port: String(port),
        },
        close: () =>
          new Promise((done) => {
            server.close(() => done());
          }),
      });
    });
  });
}

/**
 * 从请求里取端口。
 *
 * mock 自己的端口就是它允许的 Origin 端口：`http://localhost:<自己的端口>`。
 * 客户端连的是 `127.0.0.1:<端口>`，而 `Host` 头被脚本改过 —— 所以端口只能从
 * socket 上取，不能从 `Host` 头取。
 */
function portOf(request) {
  const socket = request.socket;
  return socket.localPort ?? 0;
}

/* ==========================================================================
 * 自检
 * ======================================================================== */

/**
 * `--self-test`：进程内 mock server 跑同一套断言引擎，外加一次负控。
 *
 * 负控是这一节的全部意义：断言引擎若恒真（比较写错了、状态码取错了、
 * 分支没进去），绿灯那一份照样绿。没有负控的话，「自检通过」只说明
 * 「脚本跑完了」。
 */
async function selfTest() {
  const good = await startMockServer();
  try {
    const checks = await runChecks(good.target);
    for (const check of checks) {
      console.log(check.passLine);
      if (check.note) {
        console.log(`#   ${check.note}`);
      }
    }
    const failed = checks.filter((check) => !check.ok);
    assert.equal(
      failed.length,
      0,
      `自检的 mock 全绿，六条断言却没过：\n${failed.map((c) => c.failLine).join('\n')}`,
    );
  } finally {
    await good.close();
  }

  // 负控：把 mock 的「匿名 + 代理信号」那一格故意答成 500（真实答案是 401），
  // 引擎必须把这一条判成 FAIL，并且失败行里带得出实际状态码。
  const broken = await startMockServer({ broken: 'anonymousProviders' });
  try {
    const checks = await runChecks(broken.target);
    const failed = checks.filter((check) => !check.ok);
    assert.ok(
      failed.length > 0,
      '负控失效：错误的 mock 响应没有被判 FAIL —— 断言引擎恒真，自检绿灯说明不了任何事',
    );
    const target = failed.find((check) => check.label.includes('anonymous GET /api/providers'));
    assert.ok(
      target,
      `负控失效：被判 FAIL 的不是「anonymous GET /api/providers」那一格：\n${failed
        .map((c) => c.failLine)
        .join('\n')}`,
    );
    assert.ok(
      target.failLine.includes('500'),
      `负控失效：失败行里必须带实际状态码：${target.failLine}`,
    );
  } finally {
    await broken.close();
  }

  console.log('SELF-TEST PASS：六条断言在进程内 mock 上全绿；负控（错误响应 500）被判 FAIL');
}

/* ==========================================================================
 * 入口
 * ======================================================================== */

/**
 * 读夹具。
 *
 * 优先级：环境变量 > 夹具文件。环境变量是给「对着真机跑」用的 —— 浏览器级
 * 验收时服务跑在 `pnpm tauri dev` 起的那份上，夹具文件并不存在，操作者把
 * base_url / cookie / token 填进环境变量即可。
 *
 * @returns {{baseUrl: string, sessionCookie: string, bearer: string, port: string}|null}
 */
function loadConfig() {
  const fixturePath = process.env.CC_SWITCH_E2E_FIXTURE ?? FIXTURE_PATH;
  let fromFile = null;
  let fileError = null;
  try {
    fromFile = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));
  } catch (error) {
    fileError = error;
  }

  const config = {
    baseUrl: process.env.CC_SWITCH_E2E_BASE_URL ?? fromFile?.base_url,
    sessionCookie:
      process.env.CC_SWITCH_E2E_SESSION_COOKIE ?? fromFile?.session_cookie,
    bearer: process.env.CC_SWITCH_E2E_BEARER ?? fromFile?.bearer,
  };

  const missing = Object.entries(config)
    .filter(([, value]) => typeof value !== 'string' || value.trim() === '')
    .map(([name]) => name);
  if (missing.length > 0) {
    console.log(`FAIL 读不到验收夹具：${fixturePath}`);
    if (fileError) {
      console.log(`  原因：${fileError.message}`);
    }
    console.log(`  缺的是：${missing.join(' / ')}`);
    console.log(`  起夹具：${FIXTURE_HINT}`);
    console.log(
      '  也可以直接给环境变量：CC_SWITCH_E2E_BASE_URL / CC_SWITCH_E2E_SESSION_COOKIE / CC_SWITCH_E2E_BEARER',
    );
    return null;
  }

  // 会话 cookie 的值不带 `cc_switch_session=` 前缀；操作者从浏览器里抄来的
  // 往往带着前缀，这里两种形态都收。
  const cookieValue = config.sessionCookie.includes('=')
    ? config.sessionCookie.slice(config.sessionCookie.indexOf('=') + 1)
    : config.sessionCookie;

  const url = new URL(config.baseUrl);
  return {
    baseUrl: config.baseUrl.replace(/\/+$/, ''),
    sessionCookie: cookieValue,
    bearer: config.bearer,
    port: url.port === '' ? '80' : url.port,
  };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) {
    await selfTest();
    return 0;
  }

  const config = loadConfig();
  if (config === null) {
    return 1;
  }

  console.log(`# 目标：${config.baseUrl}`);
  console.log(
    '# 说明：/api/providers 那几格都走「本机代理转发」通道（X-Forwarded-For）—— 本机直连' +
      '那一格对本机内嵌页面免凭据，不换通道的话「带凭据才有 200」这件事根本看不出来；' +
      'bearer + Host 那一格用的不是本机自己的 MagicDNS 名，否则那条断言恒真。',
  );

  const checks = await runChecks(config);
  for (const check of checks) {
    console.log(check.ok ? `PASS ${check.passLine}` : `FAIL ${check.failLine}`);
    if (check.note) {
      console.log(`#   ${check.note}`);
    }
  }

  const failed = checks.filter((check) => !check.ok);
  if (failed.length > 0) {
    console.log(`FAIL ${failed.length}/${checks.length} 条断言没过`);
    return 1;
  }
  console.log(`PASS 全部 ${checks.length} 条断言通过`);
  return 0;
}

main()
  .then((code) => {
    process.exitCode = code;
  })
  .catch((error) => {
    console.log(`FAIL 验收脚本自身出错：${error?.stack ?? error}`);
    process.exitCode = 1;
  });
