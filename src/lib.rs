//! BitrixText Forge — библиотека: Markdown → Bitrix24 BBCode.

pub mod app;
pub mod diagnostics;
pub mod highlight;
pub mod model;
pub mod parser;
pub mod profiles;
pub mod render;
pub mod settings;
pub mod storage;
pub mod tables;
pub mod templates;

pub fn build_info() -> String {
	let version = env!("CARGO_PKG_VERSION");
	let revision = option_env!("VERGEN_GIT_SHA")
		.and_then(|sha| sha.get(..7))
		.unwrap_or("unknown");
	let timestamp = option_env!("VERGEN_BUILD_TIMESTAMP").unwrap_or("unknown");
	format!("v{version} | {revision} | сборка {timestamp}")
}
