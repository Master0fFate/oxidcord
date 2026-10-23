use serde::Deserialize;

use crate::client::RestClient;
use crate::error::Result;
use crate::request::Request;

#[derive(Deserialize)]
struct GatewayInfo {
	url: String,
}

impl RestClient {
	pub async fn gateway_url(&self) -> Result<String> {
		self.send::<GatewayInfo>(Request::get("/gateway"))
			.await
			.map(|info| info.url)
	}
}
