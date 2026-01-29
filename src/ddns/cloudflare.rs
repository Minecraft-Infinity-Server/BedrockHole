use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    config::DDNSConfig,
    ddns::{DynamicDns, HTTP_CLIENT},
};

pub struct Provider {
    token: String,
    domain: String,
    sub_domain: String,
}

impl Provider {
    pub fn new(config: DDNSConfig) -> Self {
        Self {
            token: config.token,
            domain: config.domain,
            sub_domain: config.sub_domain,
        }
    }

    async fn fetch_zone_id(&self) -> anyhow::Result<String> {
        tracing::debug!(domain = %self.domain, "Fetching Cloudflare Zone ID");

        let url = format!(
            "https://api.cloudflare.com/client/v4/zones?name={}",
            self.domain
        );
        let resp: Value = HTTP_CLIENT
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await?
            .json()
            .await?;

        resp["result"]
            .as_array()
            .and_then(|list| list.get(0))
            .and_then(|zone| zone["id"].as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| anyhow::anyhow!("Zone ID not found for domain: {}", self.domain))
    }

    async fn search_record_id(
        &self,
        zone_id: &str,
        full_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records?name={}",
            zone_id, full_name
        );
        let resp: Value = HTTP_CLIENT
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .await?
            .json()
            .await?;

        Ok(resp["result"]
            .as_array()
            .and_then(|list| list.get(0))
            .and_then(|rec| rec["id"].as_str())
            .map(|id| id.to_string()))
    }

    async fn upsert_record(
        &self,
        zone_id: &str,
        rectype: &str,
        full_name: &str,
        content: &str,
        port: Option<u16>,
    ) -> anyhow::Result<()> {
        let record_id = self.search_record_id(zone_id, full_name).await?;

        let mut payload = json!({
            "type": rectype,
            "name": full_name,
            "proxied": false,
            "ttl": 60,
        });

        match rectype {
            "A" => {
                payload["content"] = json!(content);
            }
            "SRV" => {
                payload["data"] = json!({
                    "service": "_minecraft",
                    "proto": "_tcp",
                    "name": &self.sub_domain,
                    "priority": 10,
                    "weight": 0,
                    "port": port.unwrap_or(0),
                    "target": content,
                });
            }
            _ => anyhow::bail!("Unsupported record type: {}", rectype),
        }

        let (method, url) = match &record_id {
            Some(id) => (
                reqwest::Method::PATCH,
                format!(
                    "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
                    zone_id, id
                ),
            ),
            None => (
                reqwest::Method::POST,
                format!(
                    "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
                    zone_id
                ),
            ),
        };

        let resp = HTTP_CLIENT
            .request(method.clone(), url)
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
            .await?;

        if resp.status().is_success() {
            tracing::info!(
                action = %method,
                rectype = %rectype,
                name = %full_name,
                content = %content,
                "Cloudflare record synchronization successful"
            );
            Ok(())
        } else {
            let status = resp.status();
            let err_text = resp.text().await?;
            tracing::error!(
                status = %status,
                error = %err_text,
                name = %full_name,
                "Cloudflare API request failed"
            );
            anyhow::bail!("Cloudflare API error ({}): {}", status, err_text)
        }
    }
}

#[async_trait]
impl DynamicDns for Provider {
    async fn update_srv(&self, host: &str, port: u16) -> anyhow::Result<()> {
        tracing::info!(
            domain = %self.domain,
            sub_domain = %self.sub_domain,
            "Starting Cloudflare DNS synchronization"
        );

        let zone_id = self.fetch_zone_id().await?;

        let a_record_name = if self.sub_domain.is_empty() || self.sub_domain == "@" {
            self.domain.clone()
        } else {
            format!("{}.{}", self.sub_domain, self.domain)
        };

        self.upsert_record(&zone_id, "A", &a_record_name, host, None)
            .await?;

        let srv_name = format!("_minecraft._tcp.{}", a_record_name);
        self.upsert_record(&zone_id, "SRV", &srv_name, &a_record_name, Some(port))
            .await?;

        Ok(())
    }
}
