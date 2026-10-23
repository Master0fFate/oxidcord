wasmtime::component::bindgen!({
	path: "wit/oxidcord.wit",
	world: "plugin",
	additional_derives: [PartialEq],
});
