use anyhow::{anyhow, Result};
use log::info;
use serde::Deserialize;

use crate::utils::retry;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StorefrontInfo {
    pub verification: Option<VerificationInfo>,
    pub pricing: Option<PricingInfo>,
    pub is_free_software: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct VerificationInfo {
    pub verified: bool,
    pub timestamp: Option<String>,
    pub method: Option<String>,
    pub website: Option<String>,
    pub login_provider: Option<String>,
    pub login_name: Option<String>,
    pub login_is_organization: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct PricingInfo {
    pub recommended_donation: Option<i32>,
    pub minimum_payment: Option<i32>,
}

impl StorefrontInfo {
    pub fn fetch(backend_url: &str, app_id: &str) -> Result<Self> {
        let endpoint = format!("{backend_url}/purchases/storefront-info");

        let convert_err = |e| anyhow!("Failed to fetch storefront info from {}: {}", &endpoint, e);

        let client = reqwest::blocking::Client::new();

        // Fetch the storefront info
        let storefront_info = retry(|| {
            let response = client
                .get(&endpoint)
                .query(&[("app_id", app_id)])
                .send()
                .map_err(convert_err)?;

            if response.status() == 404 {
                info!("storefront-info endpoint returned 404; this must be a new app");
                return Ok(StorefrontInfo::default());
            }

            response
                .error_for_status()
                .map_err(convert_err)?
                .json::<StorefrontInfo>()
                .map_err(convert_err)
        })?;

        Ok(storefront_info)
    }
}

/// Uses a backend endpoint to determine if an app is FOSS based on its ID and license.
pub fn get_is_free_software(
    backend_url: &str,
    app_id: &str,
    license: Option<&str>,
) -> Result<bool> {
    let endpoint = format!("{backend_url}/purchases/storefront-info/is-free-software");
    let client = reqwest::blocking::Client::new();
    retry(|| {
        let mut query = vec![("app_id", app_id)];
        if let Some(license) = license {
            query.push(("license", license));
        }

        client
            .get(&endpoint)
            .query(&query)
            .send()
            .map_err(|e| anyhow!("Failed to fetch is-free-software from {}: {}", &endpoint, e))?
            .error_for_status()
            .map_err(|e| anyhow!("Failed to fetch is-free-software from {}: {}", &endpoint, e))?
            .json()
            .map_err(Into::into)
    })
}
