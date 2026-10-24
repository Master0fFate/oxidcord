wasmtime::component::bindgen!({
	path: "wit/oxidcord.wit",
	world: "plugin",
	additional_derives: [PartialEq],
});

pub mod ext {
	wasmtime::component::bindgen!({
		path: "wit/oxidcord.wit",
		world: "plugin-ext",
		additional_derives: [PartialEq],
		with: {
			"oxidcord:plugin/types": crate::bindings::oxidcord::plugin::types,
			"oxidcord:plugin/host": crate::bindings::oxidcord::plugin::host,
		},
	});
}
