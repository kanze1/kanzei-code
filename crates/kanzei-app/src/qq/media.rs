//! Typed replies own the wire payload and a byte-free delivery record.
use base64::{engine::general_purpose::STANDARD, Engine};
use kanzei_harness::permission::ScopedFile;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, io::Read, path::Path};

// Application memory budgets, not advertised QQ platform limits. Base64 is
// materialized only for the active send, never persisted in the delivery log.
pub(super) const MAX_REPLY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_ATTACHMENTS: usize = 16;
const MANIFEST: &str = "stickers/manifest.json";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplyInput {
    /// Plain QQ text. May be empty when sending only attachments.
    #[serde(default)]
    pub text: String,
    /// At most 16 attachments, 16 MiB of original bytes in total. Paths are
    /// relative to /workspace, or absolute /workspace/... sandbox paths.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Attachment {
    Image {
        path: String,
        #[serde(default)]
        summary: String,
    },
    /// Select an existing ID returned by qq_stickers. Original GIF bytes are retained.
    Sticker { id: String },
    File {
        path: String,
        #[serde(default)]
        name: Option<String>,
    },
    /// QQ's built-in emoji ID, including ID 0.
    Face { id: u32 },
    /// Reuse the actual platform metadata of a QQ marketplace sticker.
    MarketFace {
        emoji_package_id: u64,
        emoji_id: String,
        key: String,
        #[serde(default)]
        summary: String,
    },
}

pub(super) struct PreparedMessage {
    pub wire: Value,
    pub record: Value,
}

impl PreparedMessage {
    pub fn body_text(&self) -> String {
        self.wire
            .as_array()
            .into_iter()
            .flatten()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["data"]["text"].as_str())
            .collect()
    }

    pub fn text(text: &str) -> Option<Self> {
        let text = super::plain_text::render(text);
        if text.trim().is_empty() {
            return None;
        }
        let segments = json!([{"type":"text","data":{"text":text}}]);
        Some(Self {
            wire: segments.clone(),
            record: segments,
        })
    }
}

impl ReplyInput {
    pub fn prepare(self, workspace: &Path) -> anyhow::Result<PreparedMessage> {
        anyhow::ensure!(
            self.attachments.len() <= MAX_ATTACHMENTS,
            "每条 QQ 回复最多 16 个附件"
        );
        let mut wire = Vec::new();
        let mut record = Vec::new();
        let text = super::plain_text::render(&self.text);
        if !text.trim().is_empty() {
            let part = json!({"type":"text","data":{"text":text}});
            wire.push(part.clone());
            record.push(part);
        }
        let mut remaining = MAX_REPLY_BYTES;
        for attachment in self.attachments {
            let (part, receipt) = match attachment {
                Attachment::Face { id } => {
                    let part = json!({"type":"face","data":{"id":id}});
                    (part.clone(), part)
                }
                Attachment::MarketFace {
                    emoji_package_id,
                    emoji_id,
                    key,
                    summary,
                } => {
                    anyhow::ensure!(
                        emoji_package_id > 0
                            && !emoji_id.trim().is_empty()
                            && !key.trim().is_empty(),
                        "商城表情需要真实的 emoji_package_id、emoji_id 和 key"
                    );
                    let part = json!({"type":"mface","data":{"emoji_package_id":emoji_package_id,"emoji_id":emoji_id,"key":key,"summary":super::plain_text::render(&summary)}});
                    (part.clone(), part)
                }
                Attachment::Image { path, summary } => file_part(
                    workspace,
                    &path,
                    "image",
                    json!({"sub_type":0,"summary":super::plain_text::render(&summary)}),
                    &mut remaining,
                )?,
                Attachment::File { path, name } => {
                    let name = name
                        .or_else(|| Path::new(&path).file_name()?.to_str().map(str::to_owned))
                        .ok_or_else(|| anyhow::anyhow!("文件缺少名称"))?;
                    anyhow::ensure!(
                        !name.trim().is_empty()
                            && !name.contains(['/', '\\', ':'])
                            && !name.chars().any(char::is_control),
                        "文件名必须是单个可见名称"
                    );
                    file_part(
                        workspace,
                        &path,
                        "file",
                        json!({"name":name}),
                        &mut remaining,
                    )?
                }
                Attachment::Sticker { id } => {
                    let sticker = stickers(workspace)?
                        .into_iter()
                        .find(|item| item.id == id)
                        .ok_or_else(|| {
                            anyhow::anyhow!("表情包 ID 不存在：{id}；请先调用 qq_stickers")
                        })?;
                    file_part(
                        workspace,
                        &sticker.path(),
                        "image",
                        json!({"sub_type":1,"summary":sticker.name}),
                        &mut remaining,
                    )?
                }
            };
            wire.push(part);
            record.push(receipt);
        }
        anyhow::ensure!(!wire.is_empty(), "QQ 回复需要正文或至少一个附件");
        Ok(PreparedMessage {
            wire: json!(wire),
            record: json!(record),
        })
    }
}

fn file_part(
    workspace: &Path,
    path: &str,
    kind: &str,
    mut data: Value,
    remaining: &mut u64,
) -> anyhow::Result<(Value, Value)> {
    let bytes = read_file(workspace, path, *remaining)?;
    *remaining -= bytes.len() as u64;
    let receipt = json!({"type":kind,"data":data,"asset":{"path":path,"bytes":bytes.len(),"hash":kanzei_base::content_hash(&bytes)}});
    data["file"] = json!(format!("base64://{}", STANDARD.encode(bytes)));
    Ok((json!({"type":kind,"data":data}), receipt))
}

fn scoped(workspace: &Path, requested: &str) -> anyhow::Result<ScopedFile> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(workspace)?
            .file_type()
            .is_symlink(),
        "社交工作区不能是符号链接"
    );
    let relative = requested.strip_prefix("/workspace/").unwrap_or(requested);
    anyhow::ensure!(
        !relative.is_empty()
            && !relative.contains(['\\', ':'])
            && relative
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."),
        "附件路径必须在 /workspace 内，且不能包含父目录或宿主路径"
    );
    Ok(ScopedFile::resolve(
        workspace,
        workspace,
        Path::new(relative),
    )?)
}

fn read_file(workspace: &Path, path: &str, limit: u64) -> anyhow::Result<Vec<u8>> {
    let mut file = scoped(workspace, path)?.open()?;
    let metadata = file.metadata()?;
    anyhow::ensure!(metadata.is_file(), "附件必须是普通文件");
    anyhow::ensure!(
        metadata.len() > 0 && metadata.len() <= limit,
        "附件为空或超过字节预算（QQ 附件合计最多 16 MiB）"
    );
    let mut bytes = Vec::new();
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= limit,
        "读取附件时超出字节预算或文件变为空"
    );
    Ok(bytes)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    stickers: Vec<Sticker>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sticker {
    pub id: String,
    pub file: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl Sticker {
    fn path(&self) -> String {
        format!("stickers/{}", self.file)
    }
}

pub(super) fn stickers(workspace: &Path) -> anyhow::Result<Vec<Sticker>> {
    for path in [
        workspace.to_path_buf(),
        workspace.join("stickers"),
        workspace.join(MANIFEST),
    ] {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "表情库目录和清单不能是符号链接"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
    }
    let bytes = read_file(workspace, MANIFEST, MAX_MANIFEST_BYTES)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(manifest.version == 1, "表情清单 version 必须为 1");
    let mut ids = HashSet::new();
    for sticker in &manifest.stickers {
        anyhow::ensure!(
            !sticker.id.trim().is_empty()
                && !sticker.name.trim().is_empty()
                && ids.insert(&sticker.id),
            "表情 ID 和名称不能为空，ID 不能重复"
        );
        let file = scoped(workspace, &sticker.path())?.open()?;
        let meta = file.metadata()?;
        anyhow::ensure!(
            meta.is_file() && meta.len() > 0 && meta.len() <= MAX_REPLY_BYTES,
            "表情文件无效：{}",
            sticker.id
        );
    }
    Ok(manifest.stickers)
}

pub(super) fn cache_sticker(
    workspace: &Path,
    source: &kanzei_tools::image_search::ImageResult,
    bytes: &[u8],
) -> anyhow::Result<Sticker> {
    use sha2::{Digest, Sha256};
    anyhow::ensure!(!source.title.trim().is_empty(), "表情名称不能为空");
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_REPLY_BYTES,
        "表情文件为空或超过附件预算"
    );
    let media_type = kanzei_tools::image::media_type(bytes)
        .ok_or_else(|| anyhow::anyhow!("表情下载未返回支持的图片"))?;
    let id = format!("web-{:x}", Sha256::digest(bytes));
    let sticker = Sticker {
        file: format!("{id}.{}", media_type.strip_prefix("image/").unwrap()),
        id,
        name: source.title.clone(),
        description: format!(
            "{}\n来源：{}\n原图：{}",
            source.title, source.page_url, source.image_url
        ),
    };
    let mut existing = stickers(workspace)?;
    existing.retain(|item| item.id != sticker.id);
    existing.push(Sticker {
        id: sticker.id.clone(),
        file: sticker.file.clone(),
        name: sticker.name.clone(),
        description: sticker.description.clone(),
    });
    let encoded = serde_json::to_vec(&Manifest {
        version: 1,
        stickers: existing,
    })?;
    anyhow::ensure!(
        encoded.len() as u64 <= MAX_MANIFEST_BYTES,
        "表情收藏清单超过 256 KiB 预算，未写入素材"
    );
    std::fs::create_dir_all(workspace)?;
    let file = scoped(workspace, &sticker.path())?;
    file.create_parent()?;
    file.write_atomic(bytes, || Ok(()))?;
    let manifest = scoped(workspace, MANIFEST)?;
    manifest.write_atomic(&encoded, || Ok(()))?;
    Ok(sticker)
}

#[cfg(test)]
mod tests;
