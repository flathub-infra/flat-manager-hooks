use std::error::Error;

use log::info;
use serde::Deserialize;

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
    fn fetch_once(backend_url: &str, app_id: &str) -> Result<Self, Box<dyn Error>> {
        let endpoint = format!("{backend_url}/purchases/storefront-info");

        let convert_err = |e| format!("Failed to fetch storefront info from {}: {}", &endpoint, e);

        // Fetch the storefront info
        let response = reqwest::blocking::Client::new()
            .get(&endpoint)
            .query(&[("app_id", app_id)])
            .send()
            .map_err(convert_err)?;

        let storefront_info = if response.status() == 404 {
            info!("storefront-info endpoint returned 404; this must be a new app");
            Default::default()
        } else {
            response
                .error_for_status()
                .map_err(convert_err)?
                .json::<StorefrontInfo>()
                .map_err(convert_err)?
        };

        Ok(storefront_info)
    }

    pub fn fetch(backend_url: &str, app_id: &str) -> Result<Self, Box<dyn Error>> {
        let mut i = 0;

        const RETRY_COUNT: i32 = 15;
        const WAIT_TIME: u64 = 1;

        loop {
            match Self::fetch_once(backend_url, app_id) {
                Ok(info) => return Ok(info),
                Err(e) => {
                    info!("{}", e);
                    i += 1;
                    if i > RETRY_COUNT {
                        return Err(e);
                    }
                    info!("Retrying ({i}/{RETRY_COUNT}) in {WAIT_TIME} seconds...");
                    std::thread::sleep(std::time::Duration::from_secs(WAIT_TIME));
                }
            }
        }
    }
}
