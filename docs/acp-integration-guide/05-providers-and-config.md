# 05 — Provider & Config Management

ACP has two "unstable" management surfaces:

1. **LLM provider management** — `providers/list`, `providers/set`,
   `providers/disable`. Lets the client inspect and reconfigure which LLM
   provider an agent uses (API type, base URL, headers).
2. **Session config options** — `session/set_config_option`. Lets the client
   switch the model or mode/profile of a live session without rebinding.

Both require the `unstable` feature. Provider management additionally requires
the schema's `unstable_llm_providers` feature, which the SDK's `unstable`
umbrella **does not forward** in 1.2.0/1.3.0 — see the workaround below.

## Capability gate

Before any provider RPC, check that the agent advertised the providers
capability in `initialize`:

```rust
// From cached SessionCaps (see 02-session-lifecycle.md):
pub fn require_providers_supported(caps: SessionCaps) -> Result<(), MyError> {
    if caps.providers_supported {
        Ok(())
    } else {
        Err(MyError::unsupported("agent does not support the providers capability"))
    }
}
```

Send `providers/*` only when `agent_capabilities.providers.is_some()`. Otherwise
return a 501-class "unsupported" error to your UI/REST layer.

## The hand-rolled `JsonRpcRequest` workaround (providers)

**Problem:** `agent-client-protocol` 1.2.0/1.3.0 does **not** forward the schema
feature `unstable_llm_providers` through its `unstable` umbrella. Typed
`providers/list|set|disable` RPCs therefore don't go through the SDK's generated
request enum — the types are stripped.

**Solution:** Define local request/response types that implement
`JsonRpcRequest` / `JsonRpcResponse` via the SDK's derive macros, with
Go-compatible wire field names. The derive macros are re-exported from
`agent_client_protocol` (originally from `agent-client-protocol-derive`).

```rust
use agent_client_protocol::{Agent, ConnectionTo, JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Wire DTOs ---

/// One provider in `providers/list`. Accept newer schema `providerId` when
/// deserializing for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WireProviderInfo {
    #[serde(alias = "providerId")] // accept both id and providerId
    pub id: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub supported: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<WireProviderCurrent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WireProviderCurrent {
    pub api_type: String,
    pub base_url: String,
}

// --- Request/Response types with the derive macros ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/list", response = ListProvidersWireResponse)]
struct ListProvidersWireRequest {} // empty — no params

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct ListProvidersWireResponse {
    #[serde(default)]
    providers: Vec<WireProviderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/set", response = EmptyProvidersResponse)]
#[serde(rename_all = "camelCase")]
struct SetProviderWireRequest {
    id: String,
    api_type: String,
    base_url: String,
    /// Omit when empty so the wire matches Go's `omitempty` nil map.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/disable", response = EmptyProvidersResponse)]
struct DisableProviderWireRequest {
    id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonRpcResponse)]
struct EmptyProvidersResponse {}
```

**Key points:**
- `#[derive(JsonRpcRequest)]` + `#[request(method = "...", response = ...)]`
  makes the type usable with `cx.send_request(MyRequest { ... })`.
- `#[derive(JsonRpcResponse)]` marks a type as a valid RPC response.
- `#[serde(rename_all = "camelCase")]` matches ACP's JSON wire convention.
- `#[serde(alias = "providerId")]` accepts both the Go-compatible `id` and the
  newer schema's `providerId` for forward compatibility.

**Why not use the SDK's generated types?** Check whether your SDK version
forwards `unstable_llm_providers`. If it does, you can drop the hand-rolled
types and use the generated ones. The production code keeps a direct
`agent-client-protocol-schema` dep with `features = ["unstable_llm_providers"]`
for Cargo feature unification (so `InitializeResponse.agent_capabilities.providers`
isn't stripped), but still uses hand-rolled request types because the SDK's
generated request enum didn't include them in 1.2.0.

## Provider RPCs

All three use the same `cx.send_request(...).block_task().await` pattern:

```rust
pub async fn rpc_list_providers(cx: &ConnectionTo<Agent>) -> Result<Vec<ProviderInfo>, MyError> {
    let response = cx
        .send_request(ListProvidersWireRequest {})
        .block_task()
        .await
        .map_err(|e| map_rpc_error(e, "providers/list"))?;
    Ok(to_interface_providers(response.providers))
}

pub async fn rpc_set_provider(
    cx: &ConnectionTo<Agent>,
    id: String, api_type: String, base_url: String, headers: HashMap<String, String>,
) -> Result<(), MyError> {
    if id.is_empty() || api_type.is_empty() || base_url.is_empty() {
        return Err(MyError::validation("id, api_type, base_url are required"));
    }
    cx.send_request(SetProviderWireRequest { id, api_type, base_url, headers })
        .block_task().await
        .map_err(|e| map_rpc_error(e, "providers/set"))?;
    Ok(())
}

pub async fn rpc_disable_provider(cx: &ConnectionTo<Agent>, id: String) -> Result<(), MyError> {
    if id.is_empty() { return Err(MyError::validation("provider id is required")); }
    cx.send_request(DisableProviderWireRequest { id })
        .block_task().await
        .map_err(|e| map_rpc_error(e, "providers/disable"))?;
    Ok(())
}

fn map_rpc_error(error: impl std::fmt::Display, method: &'static str) -> MyError {
    tracing::error!(%error, "{method} failed");
    MyError::internal(format!("{method}: {error}"))
}
```

### Projecting to your app's DTO

Return an empty `Vec` (never `null`/`None`) so REST serializes `[]`:

```rust
pub fn to_interface_providers(providers: Vec<WireProviderInfo>) -> Vec<ProviderInfo> {
    providers.into_iter().map(|p| ProviderInfo {
        id: p.id,
        required: p.required,
        supported: p.supported,
        current: p.current.map(|c| ProviderCurrentConfig {
            api_type: c.api_type, base_url: c.base_url,
        }),
    }).collect()
}
```

## Session config options: model & profile switching

`session/set_config_option` switches a live session's model or mode/profile
without rebinding to a new agent. It's a **generated** SDK type (no hand-rolled
workaround needed).

```rust
use agent_client_protocol::schema::v1::{
    SessionId, SetSessionConfigOptionRequest,
};

/// Shared sender for model and profile switches.
async fn rpc_set_config_option(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
    config_id: &str,   // the config option id (from session/new or session/load)
    value: &str,       // the model id or profile id to select
    kind: &str,        // "model" or "profile" — for logging only
) -> Result<(), MyError> {
    let request = SetSessionConfigOptionRequest::new(
        agent_session_id.clone(),
        config_id.to_string(),
        value, // &str → SessionConfigOptionValue via From<&str>
    );
    cx.send_request(request).block_task().await.map_err(|error| {
        tracing::error!(%error, config_id, value, kind, "set_config_option failed");
        MyError::internal(format!("session/set_config_option ({kind}): {error}"))
    })?;
    Ok(())
}

pub async fn rpc_set_model_config(
    cx: &ConnectionTo<Agent>, session_id: &SessionId, config_id: &str, model_id: &str,
) -> Result<(), MyError> {
    rpc_set_config_option(cx, session_id, config_id, model_id, "model").await
}

pub async fn rpc_set_profile_config(
    cx: &ConnectionTo<Agent>, session_id: &SessionId, config_id: &str, profile_id: &str,
) -> Result<(), MyError> {
    rpc_set_config_option(cx, session_id, config_id, profile_id, "profile").await
}
```

## Config-option discovery

`NewSessionResponse` / `LoadSessionResponse` carry
`config_options: Option<Vec<SessionConfigOption>>`. To switch model/profile
live, you must first find the **config option id** for the model selector and
the profile/mode selector. Agents aren't consistent about ids/names/categories,
so use a multi-pass heuristic.

### `SessionConfigOption` shape

```rust
// agent_client_protocol::schema::v1::SessionConfigOption
// {
//     id: ConfigOptionId,           // newtype around Arc<str>
//     name: String,
//     category: Option<SessionConfigOptionCategory>, // Model | Mode | ...
//     kind: SessionConfigKind,      // Select(SelectKind) | ...
// }
//
// SessionConfigKind::Select(SessionConfigSelect) has:
//   current_value: ConfigOptionValue (newtype around Arc<str>)
//   options: SessionConfigSelectOptions
//     ::Ungrouped(Vec<ConfigOptionSelectOption>)
//     ::Grouped(Vec<ConfigOptionSelectGroup>)  // each group has .options
//     | other (non-exhaustive)
```

### Find the model config option id

Priority: category `model` → id `"model"` → name contains "model" → a value
matches a known model id.

```rust
use std::collections::HashSet;
use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions,
};

pub fn find_model_config_id(
    opts: &[SessionConfigOption],
    known_models: &[String], // your registered model ids
) -> Option<String> {
    let known: HashSet<&str> = known_models.iter().map(String::as_str).collect();

    // Pass 1: explicit category == model (spec-preferred).
    for opt in opts {
        if matches!(opt.category, Some(SessionConfigOptionCategory::Model))
            && matches!(opt.kind, SessionConfigKind::Select(_))
        {
            return Some(opt.id.to_string());
        }
    }
    // Pass 2: conventional id "model".
    for opt in opts {
        if opt.id.0.as_ref() == "model" && matches!(opt.kind, SessionConfigKind::Select(_)) {
            return Some(opt.id.to_string());
        }
    }
    // Pass 3: name contains "model" (case-insensitive).
    for opt in opts {
        if opt.name.to_ascii_lowercase().contains("model")
            && matches!(opt.kind, SessionConfigKind::Select(_))
        {
            return Some(opt.id.to_string());
        }
    }
    // Pass 4: current or listed value matches a known model id.
    if known.is_empty() { return None; }
    for opt in opts {
        let SessionConfigKind::Select(select) = &opt.kind else { continue; };
        if known.contains(select.current_value.0.as_ref()) {
            return Some(opt.id.to_string());
        }
        let hit = match &select.options {
            SessionConfigSelectOptions::Ungrouped(options) => {
                options.iter().any(|o| known.contains(o.value.0.as_ref()))
            }
            SessionConfigSelectOptions::Grouped(groups) => {
                groups.iter().flat_map(|g| g.options.iter())
                    .any(|o| known.contains(o.value.0.as_ref()))
            }
            _ => false,
        };
        if hit { return Some(opt.id.to_string()); }
    }
    None
}
```

### Find the profile (mode) config option id

Priority: category `mode` → id `"profile"`.

```rust
pub fn find_profile_config_id(opts: &[SessionConfigOption]) -> Option<String> {
    for opt in opts {
        if matches!(opt.category, Some(SessionConfigOptionCategory::Mode))
            && matches!(opt.kind, SessionConfigKind::Select(_))
        {
            return Some(opt.id.to_string());
        }
    }
    for opt in opts {
        if opt.id.0.as_ref() == "profile" && matches!(opt.kind, SessionConfigKind::Select(_)) {
            return Some(opt.id.to_string());
        }
    }
    None
}
```

### Fallback when no profile config option exists

If `find_profile_config_id` returns `None`, the agent lacks the mode/profile
capability. Inject profile instructions into the prompt context as a fallback
(prepend to the user text — see [03-prompts...](03-prompts-streaming-cancellation.md)
§"System prompt / context injection").

## When to use which surface

| Need | Use |
|------|-----|
| Change which LLM provider an agent calls | `providers/set` (then optionally `providers/disable` the old one) |
| List available/required providers | `providers/list` |
| Switch model on a live session | `session/set_config_option` with the model config id |
| Switch mode/profile on a live session | `session/set_config_option` with the profile config id |
| Change agent binary or workspace | Rebind: tear down the session and create a new one (no live RPC for this) |

**Rebind vs live switch:** `session/set_config_option` switches model/profile
on a live session. Changing the agent binary, workspace, or fundamental config
requires a full rebind (close + create new session with a new actor).
