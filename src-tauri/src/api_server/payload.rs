use serde_json::{json, Value};

/// 模型显示名 → (canonical config_name, 内部 model_name) 映射
/// 大小写不敏感：客户端可传入 "doubao-seed-2.1-turbo" 或 "Doubao-Seed-2.1-Turbo"
fn model_config(model: &str) -> (&'static str, &'static str) {
    match model.to_lowercase().as_str() {
        "deepseek-v4-flash" => ("DeepSeek-V4-Flash", "deepseek_v4_flash__dev"),
        "deepseek-v4-flash-official" => ("DeepSeek-V4-Flash-Official", "DeepSeek-V4-Flash-Official__dev"),
        "deepseek-v4-pro" => ("DeepSeek-V4-Pro", "deepseek_v4_pro__dev"),
        "glm-5.2" => ("glm-5.2", "glm-5.2__dev"),
        "glm-5.3" => ("glm-5.3", "glm-5.3__dev"),
        "doubao-seed-2.1-pro" | "seed-code-pro-0430" => ("Doubao-Seed-2.1-Pro", "Doubao-Seed-2.1-Pro__dev"),
        "doubao-seed-2.1-turbo" => ("Doubao-Seed-2.1-Turbo", "Doubao-Seed-2.1-Turbo__dev"),
        "kimi-k2.7-code" => ("kimi-k2.7-code", "kimi-k2.7-code__dev"),
        "minimax-m3" => ("minimax-m3", "minimax-m3__dev"),
        _ => ("DeepSeek-V4-Flash", "deepseek_v4_flash__dev"),
    }
}

/// 生成类似 UUID 的十六进制字符串
fn gen_uuid_like() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos();
    let seed = (nanos as u64).wrapping_mul(0x517cc1b727220a95);
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&seed.to_le_bytes());
    buf[8..16].copy_from_slice(&(seed.wrapping_add(0x9e3779b97f4a7c15)).to_le_bytes());
    let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// OpenAI 请求体 → llm_utils_chat 请求体改写
/// llm_utils_chat 消耗 IDE 积分(product_id 208)
pub fn prepare_llm_chat_body(
    src: &[u8],
    default_model: &str,
    uid: &str,
    device_id: &str,
    machine_id: &str,
) -> Vec<u8> {
    let mut obj: Value = match serde_json::from_slice(src) {
        Ok(v) => v,
        Err(_) => return src.to_vec(),
    };
    let obj_mut = match obj.as_object_mut() {
        Some(m) => m,
        None => return src.to_vec(),
    };

    // messages content string → [{type:text, text:...}]
    if let Some(msgs) = obj_mut.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for mi in msgs.iter_mut() {
            if let Some(m) = mi.as_object_mut() {
                let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();

                // assistant tool_calls: function → function_call
                if role == "assistant" {
                    if let Some(tcs) = m.get_mut("tool_calls").and_then(|t| t.as_array_mut()) {
                        let kept: Vec<Value> = tcs
                            .iter_mut()
                            .filter_map(|tc| {
                                let tcm = tc.as_object_mut()?;
                                if let Some(fn_val) = tcm.remove("function") {
                                    tcm.insert("function_call".into(), fn_val);
                                }
                                let has_name = tcm
                                    .get("function_call")
                                    .and_then(|fc| fc.get("name"))
                                    .and_then(|n| n.as_str())
                                    .map(|s| !s.trim().is_empty())
                                    .unwrap_or(false);
                                if has_name { Some(tc.clone()) } else { None }
                            })
                            .collect();
                        if kept.is_empty() {
                            m.remove("tool_calls");
                        } else {
                            *tcs = kept;
                        }
                    }
                }

                // content string → array
                if let Some(content) = m.get("content") {
                    if let Some(s) = content.as_str() {
                        m.insert(
                            "content".into(),
                            json!([{ "type": "text", "text": s }]),
                        );
                    }
                }
            }
        }
    }

    // model → config_name + model_name (大小写不敏感，规范化为 Trae 客户端使用的标准名称)
    let model = obj_mut
        .get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_model.to_string());
    let (config_name, model_name) = model_config(&model);

    // normalize tool_choice and tools (reuse existing logic)
    normalize_tool_choice(obj_mut);
    normalize_tools(obj_mut);

    // 添加 llm_utils_chat 必需字段
    obj_mut.insert("config_name".into(), json!(config_name));
    obj_mut.insert("model_name".into(), json!(model_name));
    obj_mut.insert("stream".into(), json!(true));
    obj_mut.insert("function".into(), json!(super::FUNCTION));
    obj_mut.insert("max_tokens".into(), json!(4096));
    obj_mut.insert("conversation_id".into(), json!(gen_uuid_like()));
    obj_mut.insert("user_id".into(), json!(uid));
    obj_mut.insert("session_id".into(), json!(gen_uuid_like()));
    obj_mut.insert("device_id".into(), json!(device_id));
    obj_mut.insert("machine_id".into(), json!(machine_id));
    obj_mut.insert("project_id".into(), json!(gen_uuid_like()));
    obj_mut.insert("workspace_id".into(), json!("e04cdd"));
    obj_mut.insert("prompt_max_tokens".into(), json!(168000));
    obj_mut.insert("mode".into(), json!("FunctionCall"));
    obj_mut.insert("ide_version".into(), json!(super::IDE_VERSION));
    obj_mut.insert("ide_version_code".into(), json!(super::IDE_VERSION_CODE));
    obj_mut.insert("app_id".into(), json!(super::APP_ID));
    obj_mut.insert("package_type".into(), json!("stable_cn"));

    serde_json::to_vec(&obj).unwrap_or_else(|_| src.to_vec())
}

fn normalize_tool_choice(obj: &mut serde_json::Map<String, Value>) {
    let suppress = |obj: &mut serde_json::Map<String, Value>| {
        obj.remove("tools");
        obj.remove("functions");
    };
    let tc = match obj.remove("tool_choice") {
        Some(v) => v,
        None => return,
    };
    match tc {
        Value::String(s) => {
            if s.trim().eq_ignore_ascii_case("none") {
                suppress(obj);
            } else {
                obj.insert("tool_choice".into(), Value::String(s));
            }
        }
        Value::Object(map) => {
            let typ = map
                .get("type")
                .and_then(|t| t.as_str())
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            match typ.as_str() {
                "none" => suppress(obj),
                "auto" | "required" => {
                    obj.insert("tool_choice".into(), Value::String(typ));
                }
                "function" => {
                    let name = map
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .or_else(|| map.get("name").and_then(|n| n.as_str()))
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "auto".to_string());
                    obj.insert("tool_choice".into(), Value::String(name));
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn normalize_tools(obj: &mut serde_json::Map<String, Value>) {
    let raw = match obj.get_mut("tools") {
        Some(v) => v,
        None => return,
    };
    let list = match raw.as_array_mut() {
        Some(a) => a,
        None => return,
    };
    if list.is_empty() {
        obj.remove("tools");
        return;
    }
    let mut out = Vec::new();
    for item in list.iter_mut() {
        let t = match item.as_object_mut() {
            Some(o) => o,
            None => continue,
        };
        let fn_obj = match t.get_mut("function").and_then(|f| f.as_object_mut()) {
            Some(o) => o,
            None => continue,
        };
        // parameters object → JSON string
        if let Some(params) = fn_obj.get("parameters") {
            if params.is_object() {
                if let Ok(s) = serde_json::to_string(params) {
                    fn_obj.insert("parameters".into(), Value::String(s));
                }
            }
        }
        out.push(item.clone());
    }
    if out.is_empty() {
        obj.remove("tools");
    } else {
        *raw = Value::Array(out);
    }
}
