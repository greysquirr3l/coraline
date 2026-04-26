# MCP Production Security Hardening Plan

This plan adapts production MCP security controls to the current Coraline server architecture.

## Scope

Goal: prevent sensitive data leakage, prompt injection abuse, and multi-step data exfiltration in production MCP deployments.

Current request flow:
- MCP request loop and dispatch live in crates/coraline/src/mcp.rs
- Tool execution and name normalization live in crates/coraline/src/tools/mod.rs
- Runtime configuration lives in crates/coraline/src/config.rs
- Structured logging setup lives in crates/coraline/src/logging.rs
- MCP startup wiring lives in crates/coraline/src/bin/coraline.rs

## Control Objectives

1. Redact sensitive data before it enters model-visible context.
2. Block suspicious injected instructions before they influence tool selection.
3. Enforce output guardrails before returning tool results to MCP clients.
4. Detect suspicious multi-step behavior across a session.
5. Produce auditable traces for incident response and compliance.

## Production Checklist

### Data Handling

- [ ] PII redaction enabled for tool outputs containing user, employee, credential, token, or key material.
- [ ] Redaction categories are configurable (email, phone, SSN-style ids, payment strings, access tokens).
- [ ] Raw, unredacted content is never logged.
- [ ] Redaction decisions are logged as structured events with category counts, not raw matches.

### Input Guardrails

- [ ] Input scanning on user messages and tool inputs for instruction-in-data patterns.
- [ ] Prompt override and jailbreak patterns blocked or quarantined.
- [ ] Source-aware trust mode (strict for untrusted sources).
- [ ] Risky imperative patterns (delete, transfer, send secrets, export all) flagged.

### Output Guardrails

- [ ] Output scanning before MCP response serialization.
- [ ] Tool call results validated against policy (allowed fields, response size bounds).
- [ ] High-risk operations require explicit allow policy.
- [ ] Violations return safe error payloads without sensitive internals.

### Exfiltration Prevention

- [ ] Per-session counters for read-volume and high-risk calls.
- [ ] Read-then-send and bulk-read patterns detected.
- [ ] Destination allowlist for outbound or write-like tools.
- [ ] Automatic session suspension mode for repeated high-risk events.

### Audit and Operations

- [ ] Every tool call emits structured audit event: who, tool, args hash, result size, decision.
- [ ] Guardrail decision logs are retained and searchable.
- [ ] Incident runbook documented: suspend MCP serve path, capture logs, rotate credentials.
- [ ] Regression tests cover pass and block paths for each guardrail class.

## Code-Level Integration Map

### 1) Request Entry and Session Context

Insertion point:
- crates/coraline/src/mcp.rs:130 (McpServer::start)
- crates/coraline/src/mcp.rs:163 (McpServer::handle_message)

Add:
- SessionSecurityState on McpServer for per-session counters and risk score.
- Request correlation id derived from JSON-RPC id plus monotonic sequence.

### 2) Tool Call Pre-Execution Guardrails

Insertion point:
- crates/coraline/src/mcp.rs:362 (McpServer::handle_tools_call)

Add, before registry.execute:
- Validate and classify input arguments by sensitivity and trust source.
- Input guardrail check to detect injection/jailbreak patterns in argument text fields.
- Policy engine decision: allow, redact, deny, or require confirmation (future interactive mode).

### 3) Tool Result Post-Execution Guardrails

Insertion point:
- crates/coraline/src/mcp.rs:385 (success path after registry.execute)

Add, before ToolResult serialization:
- PII redaction on result JSON/text payload.
- Output policy checks on size, prohibited patterns, and sensitive categories.
- If blocked, return safe policy error with is_error true and no sensitive payload.

### 4) Tool Policy and Classification

Insertion point:
- crates/coraline/src/tools/mod.rs:78 (ToolRegistry impl)
- crates/coraline/src/tools/mod.rs:143 (create_default_registry)

Add:
- Tool risk metadata map (read_only, write_like, network_like, memory_mutation).
- Optional per-tool destination policy for any send/write-capable tool categories.

### 5) Config Surface

Insertion point:
- crates/coraline/src/config.rs:383 (CoralineConfig)
- crates/coraline/src/config.rs:399 (load_toml_config)
- crates/coraline/src/config.rs:453 (DEFAULT_TOML_TEMPLATE)

Add:
- New security section in config.toml with defaults:
  - enabled = false (opt-in initially)
  - redaction_categories
  - input_guardrail_mode = off|monitor|enforce
  - output_guardrail_mode = off|monitor|enforce
  - session_volume_thresholds
  - allowlisted_destinations

### 6) Logging and Audit Trace

Insertion point:
- crates/coraline/src/logging.rs:31 (logging init)
- crates/coraline/src/mcp.rs lines 382, 385, 405 (tool call logs)

Add:
- Structured audit events with stable keys:
  - event = mcp_tool_call
  - decision = allow|redact|deny
  - tool_name
  - request_id
  - arg_hash
  - result_size
  - guardrail_hits
- Keep current human-readable logs, but add machine-parseable fields.

### 7) CLI and Startup Controls

Insertion point:
- crates/coraline/src/bin/coraline.rs:333-336 (Serve path)

Add:
- Startup warning when serving MCP with security disabled.
- Optional strict flag in serve args to require security.enabled = true.

## Minimal First PR (Recommended)

Deliver smallest useful security baseline in one PR.

### Features

1. Output redaction middleware in handle_tools_call success path.
2. Basic denylist output guardrail for credential-like strings.
3. Structured audit event on every tools/call.
4. Config section for security with monitor/enforce modes.

### Non-Goals

- Full NLP-based prompt injection detector.
- Cross-session anomaly detection service.
- External SIEM export.

### Suggested File Changes

- crates/coraline/src/mcp.rs
  - Add pre/post guardrail hooks around registry.execute.
  - Add audit event helper.
- crates/coraline/src/config.rs
  - Add SecurityConfig and template entries.
- crates/coraline/src/lib.rs
  - Export new security module.
- crates/coraline/src/security.rs (new)
  - Redaction, pattern checks, and policy decision types.
- crates/coraline/tests or mcp unit tests
  - Add tests for allow, redact, and deny outcomes.

### Acceptance Criteria

- Sensitive tokens and identifiers are redacted in tool outputs when security is enabled.
- Guardrail deny path returns deterministic safe error text.
- Audit logs include request id, tool name, decision, and guardrail hit count.
- Existing MCP behavior remains unchanged when security is disabled.

## Test Plan

1. Unit tests for redaction categories and denylist patterns.
2. Unit tests for policy mode behavior:
   - off: no enforcement
   - monitor: log only
   - enforce: block and redact
3. MCP handler tests for tools/call path:
   - success result redacted
   - blocked result returned as error payload
4. Run full test suite with all features:
   - cargo test --all-features

## Rollout Strategy

1. Release with monitor mode default.
2. Observe guardrail hit rates in logs.
3. Tune categories and patterns.
4. Switch production to enforce mode.
