//! R-164 批2:Embedder trait 与 openai 兼容 `/embeddings` 第一实现。
//!
//! 设计基线 docs/design/memory_control_plane.md §5 / §0.3:
//! ```text
//! trait Embedder { fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>; }
//! ```
//! - Embedder 走 provider 体系:第一实现调 openai 兼容 `/embeddings`
//!   (OpenAI / DeepSeek / 本地 Ollama 等,base_url 复用 provider 配置)。
//! - 无 embedder 时系统必须完整可用(验收①)——本模块不产生任何副作用,
//!   通道开关由 `[embeddings]` 配置节 + [`SqliteMemoryIndex`] 的降级逻辑决定。
//! - 进程内模型(ort/candle/GGUF)只做后续 benchmark challenger,绝不 bundle。
//!
//! 实现注:embeddings 是一次性 JSON 响应,不是 SSE 流,所以不走
//! `LlmClient::stream`(chat 协议),直接用 reqwest POST `/embeddings`。

use kanzei_harness::config::KanzeiConfig;
use std::sync::Arc;

/// 文本 → 向量。实现方负责 provider 协议与错误分类。
pub trait Embedder: Send + Sync {
    /// 批量向量化。返回顺序与输入一致;任一文本失败整体报错(调用方决定降级)。
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// openai 兼容 `/embeddings` 实现(R-164 内容②)。
///
/// base_url 复用 provider 配置(如 ollama=`http://127.0.0.1:11434/v1`),
/// api_key 经 provider 的 api_key_env / api_key 解析;本地服务可不带 key。
pub struct OpenAiEmbedder {
    http: reqwest::Client,
    /// 形如 `http://127.0.0.1:11434/v1`(末尾 `/embeddings` 由请求时拼)。
    base_url: String,
    model: String,
    /// Bearer token;本地服务可为空。
    api_key: Option<String>,
}

impl OpenAiEmbedder {
    /// 从 config 的 `[embeddings]` 节构造。
    /// provider 必须存在于 providers 表(否则返回错误),protocol 不强制——
    /// 只要是 openai 兼容 base_url 即可(本地 ollama 也是这种)。
    pub fn from_config(config: &KanzeiConfig) -> anyhow::Result<Self> {
        let embeddings = &config.embeddings;
        let provider_name = embeddings.provider.as_deref().ok_or_else(|| {
            anyhow::anyhow!("`[embeddings]` 未配置 provider——向量通道未启用")
        })?;
        let model = embeddings.model.as_deref().ok_or_else(|| {
            anyhow::anyhow!("`[embeddings]` 未配置 model——向量通道未启用")
        })?;
        let provider = config
            .providers
            .get(provider_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "`[embeddings].provider` 指向未知 provider `{provider_name}`;configured: {}",
                    config
                        .providers
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        let api_key = provider
            .api_key
            .clone()
            .or_else(|| {
                provider
                    .api_key_env
                    .as_deref()
                    .and_then(|env| std::env::var(env).ok())
            })
            .filter(|k| !k.is_empty());
        let base_url = provider.base_url.trim_end_matches('/').to_string();
        Ok(Self {
            http: reqwest::Client::new(),
            base_url,
            model: model.to_string(),
            api_key,
        })
    }

    /// 测试/程序化构造:直接给 base_url + model + api_key。
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
        }
    }
}

impl Embedder for OpenAiEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        // 同步 trait(设计 §5 签名)内部用 tokio 运行时驱动 reqwest。
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| anyhow::anyhow!("tokio runtime 创建失败: {e}"))?;
        runtime.block_on(self.embed_async(texts))
    }
}

impl OpenAiEmbedder {
    async fn embed_async(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });
        let mut builder = self
            .http
            .post(&url)
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        let response = builder
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("embeddings 请求失败: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "embeddings 返回 {status}: {}",
                text.chars().take(300).collect::<String>()
            ));
        }
        let payload: serde_json::Value = response.json().await?;
        let data = payload
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("embeddings 响应缺 `data` 数组: {payload}"))?;
        let mut out = Vec::with_capacity(texts.len());
        for item in data {
            let embedding = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("embeddings 条目缺 `embedding`: {item}"))?;
            let vec: Vec<f32> = embedding
                .iter()
                .map(|v| v.as_f64().map(|f| f as f32))
                .collect::<Option<_>>()
                .ok_or_else(|| anyhow::anyhow!("embedding 含非数值: {item}"))?;
            out.push(vec);
        }
        Ok(out)
    }
}

/// 内存假 Embedder(测试与离线评估用):每条文本按固定词表 hash 成向量,
/// 同一文本恒等向量(可断言检索命中),不同文本余弦可区分。
pub struct FakeEmbedder {
    dim: usize,
}

impl FakeEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for FakeEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                // 确定性 hash:每个字符贡献一个可重复的相位,保证同文本同向量。
                let mut vec = vec![0.0f32; self.dim];
                let mut seed = 0u64;
                for ch in text.chars() {
                    seed = seed.wrapping_mul(31).wrapping_add(ch as u64);
                }
                for (i, slot) in vec.iter_mut().enumerate() {
                    let phase = (seed.wrapping_mul(i as u64 + 1) % 1000) as f32 / 1000.0;
                    *slot = phase * 2.0 - 1.0;
                }
                vec
            })
            .collect())
    }
}

/// 把 config 里 `[embeddings]` 是否启用解析成 Option<Box<dyn Embedder>>。
/// 未配置/配置残缺 → None(向量通道关闭,hybrid 退化 lexical)。
pub fn embedder_from_config(config: &KanzeiConfig) -> anyhow::Result<Option<Arc<dyn Embedder>>> {
    if !config.embeddings.enabled() {
        return Ok(None);
    }
    Ok(Some(Arc::new(OpenAiEmbedder::from_config(config)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_embedder_确定性_同文本同向量_不同文本可区分() {
        let e = FakeEmbedder::new(16);
        let a = e.embed(&["old_string not found"]).unwrap();
        let b = e.embed(&["old_string not found"]).unwrap();
        let c = e.embed(&["cargo build 网络错误"]).unwrap();
        assert_eq!(a, b, "同文本必须恒等向量");
        assert_ne!(a, c, "不同文本向量必须可区分");
        assert_eq!(a[0].len(), 16);
    }

    #[test]
    fn from_config_缺节返回None_配置残缺返回错误_全配返回实例() {
        let empty = KanzeiConfig::default();
        assert!(embedder_from_config(&empty).unwrap().is_none());

        // 只配 provider 不配 model:enabled()=false → None(不报错,通道关闭)。
        let half: KanzeiConfig =
            toml::from_str("[embeddings]\nprovider = \"ollama\"\n").unwrap();
        assert!(embedder_from_config(&half).unwrap().is_none());

        // 配全但 provider 未知 → 报错(配置错误要可见,不是静默降级)。
        let bad: KanzeiConfig = toml::from_str(
            "[embeddings]\nprovider = \"nope\"\nmodel = \"m\"\n",
        )
        .unwrap();
        assert!(embedder_from_config(&bad).is_err());

        // 配全且 provider 存在 → Some。
        let mut full: KanzeiConfig =
            toml::from_str("[embeddings]\nprovider = \"ollama\"\nmodel = \"nomic-embed-text\"\n")
                .unwrap();
        full.fill_defaults();
        let got = embedder_from_config(&full).unwrap();
        assert!(got.is_some());
    }

    #[test]
    fn openai_embedder_请求与解析() {
        // 本地起一个 mock /embeddings 服务,验证 URL/请求体/响应解析。
        // 测试体本身同步:embed() 内部建 tokio runtime(与生产路径一致)。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            use std::io::Read;
            let n = socket.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            // 断言请求路径与 body 结构。
            assert!(
                req.starts_with("POST /v1/embeddings"),
                "必须 POST /embeddings: {req}"
            );
            assert!(req.contains(r#""model":"nomic-embed-text""#), "{req}");
            assert!(
                req.contains(r#""input":["hi"]"#) || req.contains(r#""input":["hi","bye"]"#),
                "{req}"
            );
            let body = r#"{"object":"list","data":[
                {"object":"embedding","index":0,"embedding":[0.1,0.2,0.3]},
                {"object":"embedding","index":1,"embedding":[0.4,0.5,0.6]}
            ],"model":"nomic-embed-text","usage":{"prompt_tokens":2,"total_tokens":2}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            use std::io::Write;
            socket.write_all(resp.as_bytes()).unwrap();
        });

        let embedder = OpenAiEmbedder::new(
            &format!("http://{addr}/v1"),
            "nomic-embed-text",
            Some("test-key".into()),
        );
        let vecs = embedder.embed(&["hi", "bye"]).unwrap();
        assert_eq!(vecs.len(), 2);
        assert_eq!(vecs[0], vec![0.1, 0.2, 0.3]);
        assert_eq!(vecs[1], vec![0.4, 0.5, 0.6]);
        server.join().unwrap();
    }
}
