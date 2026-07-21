// src-tauri/src/vault/llm/presets.rs
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub models: &'static [&'static str],
    pub default_model: &'static str,
}

pub static PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "deepseek",
        label: "Deepseek",
        base_url: "https://api.deepseek.com/v1",
        models: &[
            "deepseek-v4-flash",
            "deepseek-v4-pro",
            "deepseek-chat",
            "deepseek-reasoner",
        ],
        default_model: "deepseek-v4-flash",
    },
    ProviderPreset {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        models: &["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"],
        default_model: "gpt-5.6-sol",
    },
    ProviderPreset {
        id: "kimi",
        label: "Kimi (Moonshot)",
        base_url: "https://api.moonshot.cn/v1",
        models: &["kimi-k2.7-code", "kimi-k2.7-code-highspeed", "kimi-k2.6"],
        default_model: "kimi-k2.7-code-highspeed",
    },
    ProviderPreset {
        id: "zhipu",
        label: "智谱 BigModel",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        models: &[
            "glm-5.2",
            "glm-5.1",
            "glm-5",
            "glm-5-turbo",
            "glm-4.7",
            "glm-4.7-flash",
            "glm-4.7-flashx",
            "glm-4.6",
            "glm-4.5-air",
            "glm-4.5-airx",
            "glm-4.5-flash",
            "glm-4-long",
            "glm-4-flash-250414",
            "glm-4-flashx-250414",
        ],
        default_model: "glm-4.7-flash",
    },
    ProviderPreset {
        id: "qwen",
        label: "通义 DashScope",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        models: &["qwen-max", "qwen-plus", "qwen-turbo", "qwen-long"],
        default_model: "qwen-turbo",
    },
    ProviderPreset {
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        models: &[],
        default_model: "",
    },
    ProviderPreset {
        id: "custom",
        label: "自定义",
        base_url: "",
        models: &[],
        default_model: "",
    },
];

pub fn find_preset(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn supports_thinking_mode(provider_id: &str) -> bool {
    provider_id == "deepseek"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_contain_required_providers() {
        let ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
        for required in [
            "deepseek",
            "openai",
            "kimi",
            "zhipu",
            "qwen",
            "openrouter",
            "custom",
        ] {
            assert!(ids.contains(&required), "missing preset {required}");
        }
    }

    #[test]
    fn find_preset_returns_by_id() {
        assert_eq!(find_preset("deepseek").unwrap().label, "Deepseek");
        assert!(find_preset("bogus").is_none());
    }

    #[test]
    fn only_deepseek_uses_the_thinking_wire_parameter() {
        assert!(supports_thinking_mode("deepseek"));
        assert!(!supports_thinking_mode("openai"));
    }

    #[test]
    fn all_default_models_appear_in_models_list_or_empty() {
        for p in PRESETS {
            if p.default_model.is_empty() {
                assert!(p.models.is_empty(), "{} default empty but models not", p.id);
            } else {
                assert!(
                    p.models.contains(&p.default_model),
                    "{} default not in models",
                    p.id
                );
            }
        }
    }
}
