/**
 * base_url 的 host 是 `host`（正则片段）或其子域，可带 scheme、userinfo、端口和末尾的点；
 * `path` 是紧跟其后的路径前缀（正则片段），省略时只看 host。
 * 与后端 `codex_url_host_matches_any`（按 host 标签边界）同效：出现在中转站路径或
 * 查询串里的域名、`api.z.ai.example.com` 这类伪造后缀都不认。
 */
export function urlHostPattern(host: string, path = ""): RegExp {
  return new RegExp(
    String.raw`^\s*(?:[a-z][a-z\d+.-]*://)?(?:[^/?#@]*@)?(?:[\w-]+\.)*${host}\.?(?::\d+)?` +
      (path ? path : String.raw`(?=[/?#]|\s*$)`),
    "i",
  );
}
