use std::path::Path;
use usk_core::schema::{Skill, SkillMeta};

pub struct RegistryClient {
    base_url: String,
    client: reqwest::Client,
}

impl RegistryClient {
    pub fn new(base_url: &str) -> Self {
        RegistryClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SkillMeta>, String> {
        let url = format!("{}/api/v1/search?q={}", self.base_url, urlencode(query));
        let resp = self.client.get(&url).send().await.map_err(|e| format!("request failed: {}", e))?;
        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse failed: {}", e))?;
        let results = body["results"].as_array().ok_or("invalid response: missing results")?;
        let skills: Vec<SkillMeta> = serde_json::from_value(serde_json::Value::Array(results.clone()))
            .map_err(|e| format!("deserialize failed: {}", e))?;
        Ok(skills)
    }

    /// Search the registry for skills with an exact tag match.
    ///
    /// Part of the public `RegistryClient` API for downstream SDK consumers;
    /// not yet wired into a `usk` subcommand.
    #[allow(dead_code)]
    pub async fn search_by_tag(&self, tag: &str) -> Result<Vec<SkillMeta>, String> {
        let url = format!("{}/api/v1/search?tags={}", self.base_url, urlencode(tag));
        let resp = self.client.get(&url).send().await.map_err(|e| format!("request failed: {}", e))?;
        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse failed: {}", e))?;
        let results = body["results"].as_array().ok_or("invalid response: missing results")?;
        let skills: Vec<SkillMeta> = serde_json::from_value(serde_json::Value::Array(results.clone()))
            .map_err(|e| format!("deserialize failed: {}", e))?;
        Ok(skills)
    }

    pub async fn get_package(&self, name: &str) -> Result<SkillMeta, String> {
        let url = format!("{}/api/v1/packages/{}", self.base_url, name);
        let resp = self.client.get(&url).send().await.map_err(|e| format!("request failed: {}", e))?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("parse failed: {}", e))
        } else {
            Err(format!("package '{}' not found", name))
        }
    }

    /// List all published versions of a skill.
    ///
    /// Part of the public `RegistryClient` API for downstream SDK consumers;
    /// not yet wired into a `usk` subcommand.
    #[allow(dead_code)]
    pub async fn get_versions(&self, name: &str) -> Result<Vec<String>, String> {
        let url = format!("{}/api/v1/packages/{}/versions", self.base_url, name);
        let resp = self.client.get(&url).send().await.map_err(|e| format!("request failed: {}", e))?;
        let body: serde_json::Value = resp.json().await.map_err(|e| format!("parse failed: {}", e))?;
        let versions = body["versions"].as_array().ok_or("invalid response")?;
        let v: Vec<String> = versions.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
        Ok(v)
    }

    pub async fn download(&self, name: &str, version: &str, dest: &Path) -> Result<(), String> {
        let url = format!("{}/api/v1/packages/{}/{}/download", self.base_url, name, version);
        let resp = self.client.get(&url).send().await.map_err(|e| format!("download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("failed to download '{}' v{}: HTTP {}", name, version, resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| format!("read failed: {}", e))?;
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(dest).map_err(|e| format!("extract failed: {}", e))?;
        Ok(())
    }

    pub async fn publish(&self, skill_dir: &Path) -> Result<(), String> {
        let skill_yaml = skill_dir.join("skill.yaml");
        let content = std::fs::read_to_string(&skill_yaml).map_err(|e| format!("read skill.yaml: {}", e))?;
        let skill: Skill = serde_yaml::from_str(&content).map_err(|e| format!("parse skill.yaml: {}", e))?;

        let temp = tempfile::tempdir().map_err(|e| format!("temp dir: {}", e))?;
        let tarball = temp.path().join("skill.tar.gz");
        let file = std::fs::File::create(&tarball).map_err(|e| format!("create tarball: {}", e))?;
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        archive.append_dir_all(".", skill_dir).map_err(|e| format!("archive: {}", e))?;
        let encoder = archive.into_inner().map_err(|e| format!("finalize: {}", e))?;
        encoder.finish().map_err(|e| format!("finish: {}", e))?;

        let tarball_bytes = std::fs::read(&tarball).map_err(|e| format!("read tarball: {}", e))?;

        let url = format!("{}/api/v1/publish", self.base_url);
        let resp = self.client
            .post(&url)
            .header("Content-Type", "application/gzip")
            .header("X-Skill-Name", &skill.name)
            .header("X-Skill-Version", &skill.version)
            .body(tarball_bytes)
            .send()
            .await
            .map_err(|e| format!("publish request failed: {}", e))?;

        let status = resp.status();
        if status.is_success() {
            println!("Published '{}' v{}", skill.name, skill.version);
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(format!("publish failed ({}): {}", status, text))
        }
    }
}

fn urlencode(s: &str) -> String {
    urlencoding::encode(s).to_string()
}
