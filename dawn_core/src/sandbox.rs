use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::info;
use wasmtime::{Config, Engine, Linker, Module, Store};

const ADAPTER_METADATA_ABI_VERSION: i32 = 1;
const MAX_ADAPTER_METADATA_BYTES: usize = 16 * 1024;
const ADAPTER_METADATA_PTR_EXPORT: &str = "dawn_adapter_metadata_ptr";
const ADAPTER_METADATA_LEN_EXPORT: &str = "dawn_adapter_metadata_len";
const ADAPTER_METADATA_VERSION_EXPORT: &str = "dawn_adapter_metadata_version";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExecutionResult {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_metadata: Option<WasmAdapterMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmAdapterMetadata {
    pub version: u32,
    pub raw_json: String,
    pub parsed_json: Value,
}

/// Initialize a secure Wasmtime engine.
pub fn init_engine() -> Result<Engine> {
    info!("Initializing Wasm sandbox engine...");
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config)
}

/// Executes a compiled WebAssembly skill securely.
#[cfg(test)]
pub fn execute_skill(engine: &Engine, wasm_bytes: &[u8], function_name: &str) -> Result<String> {
    execute_skill_with_metadata(engine, wasm_bytes, function_name).map(|result| result.message)
}

/// Executes a compiled WebAssembly skill and extracts optional Dawn adapter metadata.
pub fn execute_skill_with_metadata(
    engine: &Engine,
    wasm_bytes: &[u8],
    function_name: &str,
) -> Result<SkillExecutionResult> {
    info!("Compiling Wasm skill module...");
    let module = Module::new(engine, wasm_bytes)?;

    // Set up linker and define host functions (our isolated "syscalls")
    let mut linker = Linker::new(engine);

    // Example: A host function the skill can call to log safely
    linker.func_wrap(
        "env",
        "host_log",
        |_caller: wasmtime::Caller<'_, u64>, _msg_ptr: i32, _msg_len: i32| {
            // Safe memory access logic would go here
            info!("Host intercepted log from Wasm skill");
        },
    )?;

    // Create a Store and set fuel (execution limit)
    let mut store = Store::new(engine, 0_u64);
    store.set_fuel(10_000_000)?; // 10 million instructions limit

    info!("Instantiating Wasm module...");
    let instance = linker.instantiate(&mut store, &module)?;

    info!("Executing skill function: {}", function_name);
    let func = instance.get_typed_func::<(), ()>(&mut store, function_name)?;

    // Run the specified skill function
    func.call(&mut store, ())?;

    let adapter_metadata = extract_adapter_metadata(&instance, &mut store)?;

    Ok(SkillExecutionResult {
        message: "Skill executed successfully.".to_string(),
        adapter_metadata,
    })
}

fn extract_adapter_metadata(
    instance: &wasmtime::Instance,
    store: &mut Store<u64>,
) -> Result<Option<WasmAdapterMetadata>> {
    let has_ptr = instance
        .get_func(&mut *store, ADAPTER_METADATA_PTR_EXPORT)
        .is_some();
    let has_len = instance
        .get_func(&mut *store, ADAPTER_METADATA_LEN_EXPORT)
        .is_some();
    let has_version = instance
        .get_func(&mut *store, ADAPTER_METADATA_VERSION_EXPORT)
        .is_some();

    if !has_ptr && !has_len && !has_version {
        return Ok(None);
    }
    if !(has_ptr && has_len && has_version) {
        bail!("incomplete Dawn adapter metadata ABI exports");
    }

    let ptr_func = instance
        .get_typed_func::<(), i32>(&mut *store, ADAPTER_METADATA_PTR_EXPORT)
        .context("Dawn adapter metadata ptr export has an incompatible type")?;
    let len_func = instance
        .get_typed_func::<(), i32>(&mut *store, ADAPTER_METADATA_LEN_EXPORT)
        .context("Dawn adapter metadata len export has an incompatible type")?;
    let version_func = instance
        .get_typed_func::<(), i32>(&mut *store, ADAPTER_METADATA_VERSION_EXPORT)
        .context("Dawn adapter metadata version export has an incompatible type")?;
    let memory = instance
        .get_memory(&mut *store, "memory")
        .context("Dawn adapter metadata exports require an exported memory")?;

    let version = version_func.call(&mut *store, ())?;
    let ptr = ptr_func.call(&mut *store, ())?;
    let len = len_func.call(&mut *store, ())?;

    if version != ADAPTER_METADATA_ABI_VERSION {
        bail!("unsupported Dawn adapter metadata ABI version {version}");
    }
    if ptr < 0 {
        bail!("Dawn adapter metadata pointer cannot be negative");
    }
    if len <= 0 {
        bail!("Dawn adapter metadata length must be positive");
    }
    let len = usize::try_from(len).context("Dawn adapter metadata length overflowed usize")?;
    if len > MAX_ADAPTER_METADATA_BYTES {
        bail!(
            "Dawn adapter metadata length {len} exceeds maximum {MAX_ADAPTER_METADATA_BYTES} bytes"
        );
    }

    let mut bytes = vec![0_u8; len];
    memory
        .read(&mut *store, ptr as usize, &mut bytes)
        .context("failed to read Dawn adapter metadata from Wasm memory")?;

    parse_adapter_metadata(version, &bytes).map(Some)
}

fn parse_adapter_metadata(version: i32, bytes: &[u8]) -> Result<WasmAdapterMetadata> {
    if version != ADAPTER_METADATA_ABI_VERSION {
        bail!("unsupported Dawn adapter metadata ABI version {version}");
    }
    let raw_json = std::str::from_utf8(bytes)
        .context("Dawn adapter metadata is not valid UTF-8")?
        .to_string();
    let parsed_json: Value =
        serde_json::from_str(&raw_json).context("Dawn adapter metadata is not valid JSON")?;
    if !parsed_json.is_object() {
        bail!("Dawn adapter metadata must be a JSON object");
    }
    Ok(WasmAdapterMetadata {
        version: u32::try_from(version).context("Dawn adapter metadata version overflowed u32")?,
        raw_json,
        parsed_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    #[test]
    fn execute_skill_with_metadata_preserves_legacy_skill() {
        let engine = init_engine().expect("engine should initialize");
        let wasm_bytes = STANDARD
            .decode("AGFzbQEAAAABBAFgAAADAgEABw0BCXJ1bl9za2lsbAAACgQBAgAL")
            .expect("fixture should decode");

        let result = execute_skill_with_metadata(&engine, &wasm_bytes, "run_skill")
            .expect("legacy skill should execute");

        assert_eq!(result.message, "Skill executed successfully.");
        assert!(result.adapter_metadata.is_none());
        assert_eq!(
            execute_skill(&engine, &wasm_bytes, "run_skill").expect("legacy wrapper should work"),
            "Skill executed successfully."
        );
    }

    #[test]
    fn execute_skill_with_metadata_reads_adapter_metadata_exports() {
        let engine = init_engine().expect("engine should initialize");
        let metadata = r#"{"abi":"dawn.skill.intake.adapter.metadata.v1","skillId":"demo.skill"}"#;
        let wasm = wat::parse_str(format!(
            r#"
            (module
              (memory (export "memory") 1)
              (data (i32.const 16) "{}")
              (func (export "run_skill"))
              (func (export "dawn_adapter_metadata_ptr") (result i32)
                i32.const 16)
              (func (export "dawn_adapter_metadata_len") (result i32)
                i32.const {})
              (func (export "dawn_adapter_metadata_version") (result i32)
                i32.const 1))
            "#,
            escape_wat_data(metadata),
            metadata.len()
        ))
        .expect("wat fixture should compile");

        let result = execute_skill_with_metadata(&engine, &wasm, "run_skill")
            .expect("metadata skill should execute");
        let metadata = result
            .adapter_metadata
            .expect("metadata should be extracted");

        assert_eq!(metadata.version, 1);
        assert_eq!(
            metadata.parsed_json["abi"],
            "dawn.skill.intake.adapter.metadata.v1"
        );
        assert_eq!(metadata.parsed_json["skillId"], "demo.skill");
    }

    #[test]
    fn parse_adapter_metadata_rejects_invalid_payloads() {
        assert!(parse_adapter_metadata(2, br#"{"abi":"wrong"}"#).is_err());
        assert!(parse_adapter_metadata(1, b"not-json").is_err());
        assert!(parse_adapter_metadata(1, br#""not-object""#).is_err());
    }

    fn escape_wat_data(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    }
}
