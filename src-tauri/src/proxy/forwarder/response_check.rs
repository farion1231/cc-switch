//! Success-response checks that run before a response is committed, so a
//! semantic failure inside an HTTP 2xx can still fail over.

use super::*;

pub(super) fn codex_anthropic_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("error") && value.get("error").is_none() {
        return None;
    }
    let error = value.get("error").unwrap_or(&value);
    let error_type = error.get("type").and_then(Value::as_str).unwrap_or("error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string());
    Some(format!("{error_type}: {message}"))
}

pub(super) fn responses_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let status = value.get("status").and_then(Value::as_str);
    let has_error = value.get("error").is_some_and(|error| !error.is_null());
    if !matches!(status, Some("failed" | "cancelled")) && !has_error {
        return None;
    }

    let error = value.get("error").unwrap_or(&value);
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or_else(|| status.unwrap_or("error"));
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(match status {
            Some("cancelled") => "response generation was cancelled",
            _ => "response generation failed",
        });
    Some(format!("{error_type}: {message}"))
}

/// [`Forwarder::hold_opaque_state_stream`] 看到一个完整 SSE 块之后怎么做。
pub(super) enum StreamStart {
    /// 生命周期事件（或不是 JSON），接着读。
    Pending,
    /// 有输出或别的结局：交出流。
    Commit,
    /// 输出之前上游就自证验不了请求里的密文。
    Rejected(ProxyError),
}

pub(super) fn opaque_state_stream_start(block: &str, request: &Value) -> StreamStart {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = crate::proxy::sse::strip_sse_field(line, "event") {
            named_event = Some(event.trim().to_string());
        } else if let Some(data) = crate::proxy::sse::strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }
    let Ok(value) = serde_json::from_str::<Value>(&data_lines.join("\n")) else {
        return StreamStart::Pending;
    };
    let event = named_event
        .as_deref()
        .filter(|event| !event.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");
    match event {
        "response.created" | "response.in_progress" | "response.queued" => StreamStart::Pending,
        "response.failed" | "error" => {
            let response = value.get("response").unwrap_or(&value);
            let error = response.get("error").unwrap_or(response);
            match crate::proxy::opaque_state_rectifier::in_stream_rejection(error, request) {
                Some(rejection) => StreamStart::Rejected(rejection),
                None => StreamStart::Commit,
            }
        }
        _ => StreamStart::Commit,
    }
}

/// A streaming request may receive a whole JSON document even when the gateway
/// omits `application/json`. `None` means either "not JSON" or "not complete yet";
/// a parsed document is safe to commit unless it is a semantic failure envelope.
pub(super) fn inspect_responses_json_document(buffer: &str) -> Option<Result<(), ProxyError>> {
    let trimmed = buffer.trim();
    if !matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return None;
    }
    let _: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(message) = responses_error_envelope_message(trimmed.as_bytes()) {
        if let Some(rejection) = responses_envelope_opaque_rejection(trimmed.as_bytes()) {
            return Some(Err(rejection));
        }
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream returned a 2xx failure: {message}"
        ))));
    }
    Some(Ok(()))
}

/// 有些网关先回 HTTP 200，再在输出前（或 2xx 的 Response 对象里）说验不了请求里的密文
/// （#9424）。这种失败改成上游 400，回到重试循环和 HTTP 报错一样做密文整流；
/// 其余失败照旧当转换错误，可以换下一家。
pub(super) fn opaque_rejection_before_output(error: &Value) -> Option<ProxyError> {
    crate::proxy::opaque_state_rectifier::in_stream_rejection(error, &Value::Null)
}

pub(super) fn responses_envelope_opaque_rejection(body: &[u8]) -> Option<ProxyError> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .filter(|error| !error.is_null())
        .and_then(opaque_rejection_before_output)
}

/// Inspect one complete Responses SSE block while the response is still inside
/// the retry loop. `None` means the event is lifecycle-only and priming should
/// continue; `Some(Ok(()))` means it is safe to commit/replay the stream.
pub(super) fn inspect_responses_start_event(block: &str) -> Option<Result<(), ProxyError>> {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = crate::proxy::sse::strip_sse_field(line, "event") {
            named_event = Some(event.trim().to_string());
        } else if let Some(data) = crate::proxy::sse::strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_str(&data_lines.join("\n")) {
        Ok(value) => value,
        Err(_) => return None,
    };
    let event = named_event
        .as_deref()
        .filter(|event| !event.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");

    let response = value.get("response").unwrap_or(&value);
    if matches!(
        response.get("status").and_then(Value::as_str),
        Some("failed" | "cancelled")
    ) || response.get("error").is_some_and(|error| !error.is_null())
    {
        let error = response.get("error").unwrap_or(response);
        if let Some(rejection) = opaque_rejection_before_output(error) {
            return Some(Err(rejection));
        }
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Responses upstream failed before output");
        let error_type = error
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| error.get("code").and_then(Value::as_str))
            .or_else(|| response.get("status").and_then(Value::as_str))
            .unwrap_or("upstream_error");
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream {error_type}: {message}"
        ))));
    }

    match event {
        "response.failed" | "error" => {
            let error = response.get("error").unwrap_or(response);
            if let Some(rejection) = opaque_rejection_before_output(error) {
                return Some(Err(rejection));
            }
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("Responses upstream emitted an error before output");
            let error_type = error
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| error.get("code").and_then(Value::as_str))
                .unwrap_or("upstream_error");
            Some(Err(ProxyError::TransformError(format!(
                "Responses upstream {error_type}: {message}"
            ))))
        }
        "response.created" | "response.in_progress" | "response.queued" => None,
        "" => None,
        // Productive output, incomplete, and completed terminals are all safe to
        // expose. Mid-stream failures after this point are surfaced by the converter
        // but intentionally do not switch providers.
        _ => Some(Ok(())),
    }
}

impl RequestForwarder {
    /// 故障转移开启时，成功不能只看上游响应头。
    ///
    /// - 非流式：先把完整 body 读到内存，读超时/连接中断会回到 retry loop 尝试下一家。
    /// - 流式：至少等首个 chunk 到达，避免上游返回 200 后一直不吐 SSE 时被误记成功。
    pub(super) async fn prepare_success_response_for_failover(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
    ) -> Result<ProxyResponse, ProxyError> {
        if request_is_streaming {
            return self.prime_streaming_response(response).await;
        }

        if self.non_streaming_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let body_timeout = self.non_streaming_timeout;
        let body = tokio::time::timeout(
            body_timeout,
            response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES),
        )
        .await
        .map_err(|_| {
            ProxyError::Timeout(format!(
                "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                body_timeout.as_secs()
            ))
        })??;

        Ok(ProxyResponse::buffered(status, headers, body))
    }

    /// Some Anthropic-compatible gateways return an Anthropic error envelope with
    /// HTTP 2xx. Validate it inside the retry loop so the request can fail over to
    /// the next provider; the response transformer runs too late for that.
    pub(super) async fn validate_codex_anthropic_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
        let decoded = match encoding {
            Some(encoding) => {
                match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                    Ok(Some(decompressed)) => decompressed,
                    _ => raw.to_vec(),
                }
            }
            None => raw.to_vec(),
        };

        if let Some(message) = codex_anthropic_error_envelope_message(&decoded) {
            return Err(ProxyError::TransformError(format!(
                "Anthropic upstream returned a 2xx error envelope: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    pub(super) async fn validate_responses_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
        let decoded = match encoding {
            Some(encoding) => {
                match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                    Ok(Some(decompressed)) => decompressed,
                    _ => raw.to_vec(),
                }
            }
            None => raw.to_vec(),
        };

        if let Some(message) = responses_error_envelope_message(&decoded) {
            if let Some(rejection) = responses_envelope_opaque_rejection(&decoded) {
                return Err(rejection);
            }
            return Err(ProxyError::TransformError(format!(
                "Responses upstream returned a 2xx failure: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    pub(super) async fn validate_responses_stream_start(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        const MAX_PRIME_BYTES: usize = 256 * 1024;

        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut replay_chunks: Vec<Bytes> = Vec::new();
        let mut sse = crate::proxy::sse::SseBuffer::new();

        loop {
            let next = if self.streaming_first_byte_timeout.is_zero() {
                stream.next().await
            } else {
                tokio::time::timeout(self.streaming_first_byte_timeout, stream.next())
                    .await
                    .map_err(|_| {
                        ProxyError::Timeout(format!(
                            "Responses stream produced no semantic output within {}s",
                            self.streaming_first_byte_timeout.as_secs()
                        ))
                    })?
            };

            let Some(chunk) = next else {
                if let Some(outcome) = inspect_responses_json_document(sse.pending()) {
                    outcome?;
                    let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
                if !sse.pending().trim().is_empty() {
                    if let Some(outcome) = inspect_responses_start_event(sse.pending().trim()) {
                        outcome?;
                        let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                        return Ok(ProxyResponse::streamed(status, headers, replay));
                    }
                }
                return Err(ProxyError::ForwardFailed(
                    "Responses stream ended before producing output or a terminal event"
                        .to_string(),
                ));
            };
            let chunk = chunk.map_err(|error| {
                ProxyError::ForwardFailed(format!(
                    "Failed while validating Responses stream start: {error}"
                ))
            })?;
            sse.push_bytes(&chunk);
            replay_chunks.push(chunk);

            // Some compatible gateways ignore `stream:true` and return a complete
            // Responses JSON document without a JSON content-type. Recognize that
            // shape before looking for SSE delimiters; pretty-printed JSON may itself
            // contain blank lines and must stay intact.
            if let Some(outcome) = inspect_responses_json_document(sse.pending()) {
                outcome?;
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }

            while let Some(block) = sse.next_block() {
                if let Some(outcome) = inspect_responses_start_event(block) {
                    outcome?;
                    let replay =
                        futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
            }

            if replay_chunks.iter().map(Bytes::len).sum::<usize>() >= MAX_PRIME_BYTES {
                log::warn!(
                    "[Claude/Responses] semantic stream priming exceeded {MAX_PRIME_BYTES} bytes; committing buffered stream"
                );
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }
        }
    }

    /// 原样转发的 Responses 请求带着上游签发的状态时（推理条目、加密片段、压缩条目，见
    /// `opaque_state_rectifier::carries_opaque_state`），先看到第一个不是生命周期的事件
    /// 再交出流：上游解不开密文时会先回 HTTP 200、再在输出之前回 `response.failed`（#9476：
    /// 换了一家续接旧会话），这时改成上游错误，回到重试循环里和 HTTP 报错一样整流后重试，
    /// 子 agent 任务解不开时说明原因。其余情况（有输出、别的失败、到了截止
    /// 时间、缓冲太多）把读过的字节原样接回去，客户端看到的流和没经过这里一样。
    ///
    /// 整个缓冲阶段只有一个截止时间（不是每读一块重新计时）：上游在输出前一直发心跳时，流
    /// 也得按时交出去。解不开密文是上游一收到请求就报的错，不用等到生成，所以最多压
    /// [`MAX_HOLD`]，首包超时更短时按首包超时。
    pub(super) async fn hold_opaque_state_stream(
        &self,
        response: ProxyResponse,
        request: &Value,
    ) -> Result<ProxyResponse, ProxyError> {
        const MAX_HOLD_BYTES: usize = 256 * 1024;
        const MAX_HOLD: std::time::Duration = std::time::Duration::from_secs(10);

        let window = match self.streaming_first_byte_timeout {
            timeout if timeout.is_zero() => MAX_HOLD,
            timeout => timeout.min(MAX_HOLD),
        };
        let deadline = tokio::time::Instant::now() + window;
        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut replay_chunks: Vec<Bytes> = Vec::new();
        let mut held_bytes = 0usize;
        let mut sse = crate::proxy::sse::SseBuffer::new();

        'hold: loop {
            let Ok(next) = tokio::time::timeout_at(deadline, stream.next()).await else {
                break 'hold;
            };
            let Some(chunk) = next else {
                break 'hold;
            };
            // 还没有交出流，读出错和首包前出错一样回到重试循环。
            let chunk = chunk
                .map_err(|error| ProxyError::ForwardFailed(format!("读取流式响应失败: {error}")))?;
            sse.push_bytes(&chunk);
            held_bytes += chunk.len();
            replay_chunks.push(chunk);

            while let Some(block) = sse.next_block() {
                match opaque_state_stream_start(block, request) {
                    StreamStart::Pending => {}
                    StreamStart::Commit => break 'hold,
                    StreamStart::Rejected(error) => return Err(error),
                }
            }
            if held_bytes > MAX_HOLD_BYTES {
                break 'hold;
            }
        }

        let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
        Ok(ProxyResponse::streamed(status, headers, replay))
    }

    pub(super) async fn prime_streaming_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        if self.streaming_first_byte_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let timeout = self.streaming_first_byte_timeout;
        let mut stream = Box::pin(response.bytes_stream());

        let first = tokio::time::timeout(timeout, stream.next())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "流式响应首包超时: {}s（上游已返回响应头但未返回数据）",
                    timeout.as_secs()
                ))
            })?;

        let Some(first) = first else {
            return Err(ProxyError::ForwardFailed(
                "流式响应在首包到达前结束".to_string(),
            ));
        };

        let first =
            first.map_err(|e| ProxyError::ForwardFailed(format!("读取流式响应首包失败: {e}")))?;

        let replay = futures::stream::once(async move { Ok(first) }).chain(stream);
        Ok(ProxyResponse::streamed(status, headers, replay))
    }

    /// Hold back a 2xx response until it is known not to be a semantic
    /// failure, so such a failure stays inside the retry loop.
    pub(super) async fn validate_success_response(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
        codex_responses_to_anthropic: bool,
        resolved_claude_api_format: Option<&str>,
        opaque_state_request: Option<&Value>,
    ) -> Result<ProxyResponse, ProxyError> {
        let mut response = self
            .prepare_success_response_for_failover(response, request_is_streaming)
            .await?;
        // Streaming requests normally return SSE. If a compatible gateway
        // explicitly returns JSON instead, buffer and validate it inside the retry
        // loop as well so a 2xx Anthropic error envelope can still fail over. Do
        // not buffer unknown content types: some gateways omit the SSE header.
        if codex_responses_to_anthropic && (!request_is_streaming || response.is_json()) {
            response = self
                .validate_codex_anthropic_success_response(response)
                .await?;
        } else if matches!(resolved_claude_api_format, Some("openai_responses")) {
            if !request_is_streaming || response.is_json() {
                // Claude→Responses gateways can also return a semantic failure in an
                // HTTP 2xx Response object. Validate buffered/JSON bodies inside the
                // retry loop so an early failure can still select another provider.
                response = self.validate_responses_success_response(response).await?;
            } else {
                // Delay committing the downstream stream until the upstream emits
                // either productive output or a valid non-failure terminal event.
                // A response.failed/error before output remains failover-safe.
                response = self.validate_responses_stream_start(response).await?;
            }
        } else if let Some(request) = opaque_state_request.filter(|_| !response.is_json()) {
            response = self.hold_opaque_state_stream(response, request).await?;
        }
        Ok(response)
    }
}

/// The `UpstreamError` for a non-2xx response, with its (decoded, size-capped)
/// body kept when it is UTF-8.
pub(super) async fn upstream_error(response: ProxyResponse) -> Result<ProxyError, ProxyError> {
    let status_code = response.status().as_u16();
    // 错误响应同样可能被上游压缩（content-encoding）。reqwest 未启用任何
    // 自动解压 feature，这里拿到的是原始字节；不解压的话，压缩过的错误体会
    // 在 from_utf8 处变成非 UTF-8 而被丢弃，隐藏掉上游的限流/鉴权等详情。
    let encoding = get_content_encoding(response.headers());
    let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
    let decoded = match encoding {
        Some(encoding) => {
            match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                Ok(Some(decompressed)) => decompressed,
                // 不支持的编码 / 解压失败 / 解压后超限：退回（已有上限的）
                // 原始字节，尽量保留可读信息
                _ => raw.to_vec(),
            }
        }
        None => raw.to_vec(),
    };
    let body_text = String::from_utf8(decoded).ok();

    Ok(ProxyError::UpstreamError {
        status: status_code,
        body: body_text,
    })
}
