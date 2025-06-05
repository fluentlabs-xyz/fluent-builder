use eyre::Result;
use serde_json::Value;

/// Solidity ABI represented as JSON values
pub type Abi = Vec<Value>;

/// Generates ABI from parsed routers
pub fn generate(routers: &[fluentbase_sdk_derive_core::router::Router]) -> Result<Abi> {
    if routers.is_empty() {
        return Ok(Vec::new());
    }

    // Take first router for now
    let router = &routers[0];
    let mut entries = Vec::new();

    for method in router.available_methods() {
        if let Ok(func_abi) = method.parsed_signature().function_abi() {
            if let Ok(json) = func_abi.to_json_value() {
                entries.push(json);
            }
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_routers() {
        let abi = generate(&[]).unwrap();
        assert!(abi.is_empty());
    }
}
