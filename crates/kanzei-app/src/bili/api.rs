//! Bilibili web API adapter. Inspired by dsh-bili-whale's API contracts; no DSH runtime.
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};

#[derive(Debug, Clone)]
pub(crate) enum Error {
    Transport,
    Http(u16),
    Platform(i64),
    Input(String),
}
impl Error {
    pub fn uncertain(&self) -> bool {
        matches!(self, Self::Transport | Self::Http(500..=599))
    }
    pub fn pause(&self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Http(412 | 429) | Self::Platform(-101 | -352 | -412 | -509)
        )
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport => f.write_str("B站连接中断或响应无效，结果未确认；未自动重试"),
            Self::Http(code) => write!(f, "B站 HTTP {code}，请求已停止"),
            Self::Platform(code) => write!(f, "B站接口返回错误 {code}，请求已停止"),
            Self::Input(message) => f.write_str(message),
        }
    }
}
impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Self::Input(s.into())
    }
}
impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::Input(s)
    }
}
impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.to_string()
    }
}

#[derive(Clone)]
pub(crate) struct Api {
    http: reqwest::Client,
    api: String,
    passport: String,
}
impl Api {
    pub fn new() -> Result<Self, Error> {
        let endpoints = (
            "https://api.bilibili.com".to_string(),
            "https://passport.bilibili.com".to_string(),
        );
        #[cfg(debug_assertions)]
        let endpoints = std::env::var("KANZEI_E2E_BILI_ENDPOINT")
            .map(|endpoint| (endpoint.clone(), endpoint))
            .unwrap_or(endpoints);
        let (api, passport) = endpoints;
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.to_string())?,
            api,
            passport,
        })
    }
    pub async fn request(
        &self,
        path: &str,
        cookie: &str,
        params: &[(String, String)],
        post: bool,
    ) -> Result<(Value, Vec<String>), Error> {
        let base = if path.starts_with("/x/passport-login/") {
            &self.passport
        } else {
            &self.api
        };
        let request = if post {
            self.http.post(format!("{base}{path}")).form(params)
        } else {
            self.http.get(format!("{base}{path}")).query(params)
        };
        let response=request.header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0.0.0 Safari/537.36").header("Referer","https://www.bilibili.com/").header("Cookie",cookie).send().await.map_err(|_|Error::Transport)?;
        if !response.status().is_success() {
            return Err(Error::Http(response.status().as_u16()));
        }
        let cookies = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "B站登录凭据格式错误")?;
        let value: Value = response.json().await.map_err(|_| Error::Transport)?;
        let code = value["code"].as_i64().ok_or(Error::Transport)?;
        if code != 0 {
            return Err(Error::Platform(code));
        }
        Ok((value["data"].clone(), cookies))
    }
    pub async fn get(
        &self,
        path: &str,
        cookie: &str,
        params: &[(String, String)],
    ) -> Result<Value, Error> {
        Ok(self.request(path, cookie, params, false).await?.0)
    }
    pub async fn account(&self, cookie: &str) -> Result<Value, Error> {
        let nav = self.get("/x/web-interface/nav", cookie, &[]).await?;
        if nav["isLogin"] != true || nav["mid"].as_u64().is_none_or(|v| v == 0) {
            return Err(Error::Platform(-101));
        }
        Ok(nav)
    }
    pub async fn video(&self, cookie: &str, bvid: &str) -> Result<Value, Error> {
        validate_bvid(bvid)?;
        self.get(
            "/x/web-interface/view",
            cookie,
            &[("bvid".into(), bvid.into())],
        )
        .await
    }
    pub async fn feed(&self, cookie: &str, keyword: &str, page: u32) -> Result<Value, Error> {
        if page == 0 {
            return Err("页码必须大于零".into());
        }
        if keyword.trim().is_empty() {
            self.get(
                "/x/web-interface/popular",
                cookie,
                &[("ps".into(), "12".into()), ("pn".into(), page.to_string())],
            )
            .await
        } else {
            let nav = self.get("/x/web-interface/nav", cookie, &[]).await?;
            let params = signed_params(
                &nav,
                vec![
                    ("keyword".into(), keyword.into()),
                    ("search_type".into(), "video".into()),
                    ("page".into(), page.to_string()),
                ],
                chrono::Utc::now().timestamp(),
            )?;
            self.get("/x/web-interface/wbi/search/type", cookie, &params)
                .await
        }
    }
    pub async fn comments(
        &self,
        cookie: &str,
        bvid: &str,
        page: u32,
        root: Option<u64>,
    ) -> Result<Value, Error> {
        if page == 0 {
            return Err("页码必须大于零".into());
        }
        let video = self.video(cookie, bvid).await?;
        let aid = video["aid"].as_u64().ok_or("视频缺少 aid")?;
        let mut params = vec![
            ("type".into(), "1".into()),
            ("oid".into(), aid.to_string()),
            ("pn".into(), page.to_string()),
            ("ps".into(), "20".into()),
            ("sort".into(), "2".into()),
        ];
        let path = if let Some(root) = root {
            params.push(("root".into(), root.to_string()));
            "/x/v2/reply/reply"
        } else {
            "/x/v2/reply"
        };
        let data = self.get(path, cookie, &params).await?;
        Ok(json!({"aid":aid,"bvid":bvid,"comments":data}))
    }
    pub async fn transcript(&self, cookie: &str, bvid: &str) -> Result<Value, Error> {
        let video = self.video(cookie, bvid).await?;
        let cid = video["cid"].as_u64().ok_or("视频缺少 cid")?;
        let nav = self.get("/x/web-interface/nav", cookie, &[]).await?;
        let params = signed_params(
            &nav,
            vec![
                ("bvid".into(), bvid.into()),
                ("cid".into(), cid.to_string()),
            ],
            chrono::Utc::now().timestamp(),
        )?;
        let player = self.get("/x/player/wbi/v2", cookie, &params).await?;
        let tracks = player["subtitle"]["subtitles"]
            .as_array()
            .ok_or("B站字幕列表格式错误")?;
        if tracks.is_empty() {
            return Ok(
                json!({"video":video,"transcript":null,"modality":"视频信息；该视频没有可用字幕，未分析画面或音频"}),
            );
        }
        let url = tracks[0]["subtitle_url"].as_str().ok_or("字幕缺少地址")?;
        let url = if url.starts_with("//") {
            format!("https:{url}")
        } else {
            url.into()
        };
        let parsed = reqwest::Url::parse(&url).map_err(|_| "字幕地址错误")?;
        if parsed.scheme() != "https"
            || !parsed
                .host_str()
                .is_some_and(|h| h == "hdslb.com" || h.ends_with(".hdslb.com"))
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("字幕地址不属于 B站字幕服务".into());
        }
        let transcript: Value = self
            .http
            .get(parsed)
            .header("Referer", "https://www.bilibili.com/")
            .send()
            .await
            .map_err(|_| "字幕读取失败")?
            .error_for_status()
            .map_err(|_| "字幕读取失败")?
            .json()
            .await
            .map_err(|_| "字幕 JSON 无效")?;
        Ok(
            json!({"video":video,"transcript":transcript["body"],"language":tracks[0]["lan_doc"],"modality":"视频信息和字幕；未分析画面或音频"}),
        )
    }
    pub async fn publish(
        &self,
        cookie: &str,
        aid: u64,
        text: &str,
        root: Option<u64>,
        parent: Option<u64>,
    ) -> Result<Value, Error> {
        let csrf = cookie_value(cookie, "bili_jct").ok_or("B站登录缺少 csrf 凭据，请重新登录")?;
        let mut params = vec![
            ("type".into(), "1".into()),
            ("oid".into(), aid.to_string()),
            ("message".into(), text.into()),
            ("plat".into(), "1".into()),
            ("csrf".into(), csrf.into()),
        ];
        if let Some(root) = root {
            params.push(("root".into(), root.to_string()));
            params.push(("parent".into(), parent.unwrap_or(root).to_string()));
        }
        Ok(self
            .request("/x/v2/reply/add", cookie, &params, true)
            .await?
            .0)
    }
}
pub(crate) fn validate_bvid(bvid: &str) -> Result<(), Error> {
    if bvid.len() != 12
        || !bvid.starts_with("BV")
        || !bvid[2..].bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err("需要完整的 BV 视频编号".into());
    }
    Ok(())
}
pub(crate) fn cookie_value<'a>(cookie: &'a str, name: &str) -> Option<&'a str> {
    cookie
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find_map(|(k, v)| (k == name).then_some(v))
}
pub(crate) fn validate_cookie(cookie: &str) -> Result<(), Error> {
    if cookie.contains(['\r', '\n'])
        || cookie_value(cookie, "SESSDATA").is_none_or(str::is_empty)
        || cookie_value(cookie, "bili_jct").is_none_or(str::is_empty)
    {
        return Err("B站登录凭据无效".into());
    }
    Ok(())
}
fn signed_params(
    nav: &Value,
    params: Vec<(String, String)>,
    timestamp: i64,
) -> Result<Vec<(String, String)>, Error> {
    const MIXIN: [usize; 64] = [
        46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19,
        29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4,
        22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
    ];
    let key = |name: &str| -> Result<String, Error> {
        let url = nav["wbi_img"][name]
            .as_str()
            .ok_or("B站未返回 WBI 签名信息")?;
        let stem = url
            .rsplit('/')
            .next()
            .and_then(|s| s.split('.').next())
            .ok_or("WBI 图片地址无效")?;
        Ok(stem.into())
    };
    let raw = format!("{}{}", key("img_url")?, key("sub_url")?);
    if raw.len() != 64 || !raw.is_ascii() {
        return Err("WBI 签名密钥格式无效".into());
    }
    let mixin: String = MIXIN[..32]
        .iter()
        .map(|i| raw.as_bytes()[*i] as char)
        .collect();
    let mut sorted: BTreeMap<String, String> = params.into_iter().collect();
    sorted.insert("wts".into(), timestamp.to_string());
    for value in sorted.values_mut() {
        value.retain(|c| !"!'()*".contains(c));
    }
    let encode = |s: &str| -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    };
    let query = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let digest = format!("{:x}", Md5::digest(format!("{query}{mixin}").as_bytes()));
    sorted.insert("w_rid".into(), digest);
    Ok(sorted.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wbi_signature_matches_reference_with_unicode_and_filtered_punctuation() {
        let nav = json!({"wbi_img":{"img_url":"https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png","sub_url":"https://i0.hdslb.com/bfs/wbi/4932caff0ff746eab6f01bf08b70ac45.png"}});
        let signed = signed_params(
            &nav,
            vec![
                ("keyword".into(), "中文 !test*".into()),
                ("search_type".into(), "video".into()),
                ("page".into(), "1".into()),
            ],
            1_702_204_169,
        )
        .unwrap();
        let signed: BTreeMap<_, _> = signed.into_iter().collect();
        assert_eq!(signed["w_rid"], "07d6d1f7c65029797a935e6dcd00ad6a");
        assert_eq!(signed["keyword"], "中文 test");
        assert!(signed_params(&json!({}), vec![], 0).is_err());
    }
    #[test]
    fn credential_and_identity_validation_rejects_injection_and_unknown_sends_are_not_definite_failures(
    ) {
        assert!(validate_cookie("SESSDATA=abc; bili_jct=def").is_ok());
        assert!(validate_cookie("SESSDATA=abc\r\nHost: evil; bili_jct=def").is_err());
        for value in ["", "BV123", "BV123456789/", "BV中文中文中文"] {
            assert!(validate_bvid(value).is_err());
        }
        assert!(Error::Transport.uncertain());
        assert!(Error::Http(503).uncertain());
        assert!(!Error::Platform(-412).uncertain());
        assert!(Error::Platform(-412).pause());
    }
    #[tokio::test]
    #[ignore = "requires KANZEI_BILI_LIVE_COOKIE from an owner login and live Bilibili connectivity; read-only"]
    async fn live_account_feed_video_comments_and_subtitle_contract() {
        let cookie =
            std::env::var("KANZEI_BILI_LIVE_COOKIE").expect("owner login cookie is required");
        validate_cookie(&cookie).unwrap();
        let api = Api::new().unwrap();
        api.account(&cookie).await.unwrap();
        let feed = api.feed(&cookie, "", 1).await.unwrap();
        let bvid = feed["list"][0]["bvid"]
            .as_str()
            .expect("popular feed must contain a video");
        api.video(&cookie, bvid).await.unwrap();
        api.comments(&cookie, bvid, 1, None).await.unwrap();
        api.transcript(&cookie, bvid).await.unwrap();
    }
}
