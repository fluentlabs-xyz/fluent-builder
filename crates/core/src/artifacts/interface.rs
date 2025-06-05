//! Solidity interface generation from ABI

use super::abi::Abi;
use convert_case::{Case, Casing};
use eyre::Result;
use serde_json::Value;
use std::collections::HashSet;

/// Generates a Solidity interface from contract ABI
pub fn generate(contract_name: &str, abi: &Abi) -> Result<String> {
    let mut interface = String::new();

    // Header
    interface.push_str("// SPDX-License-Identifier: MIT\n");
    interface.push_str("// Auto-generated from Rust source\n");
    interface.push_str("pragma solidity ^0.8.0;\n\n");
    interface.push_str(&format!("interface I{} {{\n", contract_name.to_case(Case::Pascal)));

    // Extract and add struct definitions
    let mut seen_structs = HashSet::new();
    let mut struct_definitions = Vec::new();

    for entry in abi.iter().filter(|e| e["type"] == "function") {
        if let Some(inputs) = entry.get("inputs").and_then(Value::as_array) {
            collect_structs(inputs, &mut seen_structs, &mut struct_definitions);
        }
        if let Some(outputs) = entry.get("outputs").and_then(Value::as_array) {
            collect_structs(outputs, &mut seen_structs, &mut struct_definitions);
        }
    }

    // Add structs to interface
    if !struct_definitions.is_empty() {
        for struct_def in &struct_definitions {
            interface.push_str(struct_def);
            interface.push_str("\n\n");
        }
    }

    // Add functions
    for func in abi.iter().filter(|e| e["type"] == "function") {
        interface.push_str("    ");
        interface.push_str(&format_function(func)?);
        interface.push('\n');
    }

    interface.push_str("}\n");
    Ok(interface)
}

fn format_function(func: &Value) -> Result<String> {
    let name = func["name"].as_str().unwrap_or_default();
    let empty_vec = Vec::new();
    let inputs = func["inputs"].as_array().unwrap_or(&empty_vec);
    let outputs = func["outputs"].as_array().unwrap_or(&empty_vec);
    let mutability = func["stateMutability"].as_str().unwrap_or("nonpayable");

    let params = inputs.iter().map(format_parameter).collect::<Vec<_>>().join(", ");

    let returns = if outputs.is_empty() {
        String::new()
    } else {
        let ret_params = outputs.iter().map(format_parameter).collect::<Vec<_>>().join(", ");
        format!(" returns ({ret_params})")
    };

    let mut_str = match mutability {
        "pure" => " pure",
        "view" => " view",
        "payable" => " payable",
        _ => "",
    };

    Ok(format!("function {name}({params}) external{mut_str}{returns};"))
}

fn format_parameter(param: &Value) -> String {
    let name = param["name"].as_str().unwrap_or("");
    let internal_type = param.get("internalType").and_then(Value::as_str);

    // Use internal type for structs, otherwise use regular type
    let ty = if let Some(internal) = internal_type {
        if let Some(struct_name) = internal.strip_prefix("struct ") {
            struct_name.to_string()
        } else {
            format_sol_type(param)
        }
    } else {
        format_sol_type(param)
    };

    // Add data location for complex types
    let location = get_data_location(&ty, internal_type);
    let location_str = match location {
        Some(DataLocation::Memory) => " memory",
        Some(DataLocation::Calldata) => " calldata",
        None => "",
    };

    if name.is_empty() {
        format!("{ty}{location_str}")
    } else {
        format!("{ty}{location_str} {name}")
    }
}

fn format_sol_type(param: &Value) -> String {
    let param_type = param["type"].as_str().unwrap_or("unknown");

    if param_type == "tuple" {
        // Check if it's a named struct
        if let Some(internal_type) = param.get("internalType").and_then(Value::as_str) {
            if let Some(stripped) = internal_type.strip_prefix("struct ") {
                return stripped.to_string();
            }
        }

        // Handle anonymous tuples
        if let Some(components) = param.get("components").and_then(Value::as_array) {
            let component_types =
                components.iter().map(format_sol_type).collect::<Vec<_>>().join(",");
            format!("({component_types})")
        } else {
            "tuple".to_string()
        }
    } else if let Some(base_type) = param_type.strip_suffix("[]") {
        // Handle array types
        let formatted_base = format_sol_type(&serde_json::json!({ "type": base_type }));
        format!("{formatted_base}[]")
    } else {
        // Return primitive types as-is
        param_type.to_string()
    }
}

#[derive(Debug, Clone, Copy)]
enum DataLocation {
    Memory,
    Calldata,
}

fn get_data_location(ty: &str, internal_type: Option<&str>) -> Option<DataLocation> {
    match (ty, internal_type) {
        (_, Some(t)) if t.starts_with("struct ") => Some(DataLocation::Memory),
        ("string", _) | ("bytes", _) => Some(DataLocation::Calldata),
        (t, _) if t.ends_with("[]") => Some(DataLocation::Memory),
        (t, _) if t.starts_with("(") && t.ends_with(")") => Some(DataLocation::Memory), // tuples
        _ => None,
    }
}

fn collect_structs(params: &[Value], seen: &mut HashSet<String>, structs: &mut Vec<String>) {
    for param in params {
        if param["type"] == "tuple" {
            if let Some(internal_type) = param.get("internalType").and_then(Value::as_str) {
                if let Some(struct_name) = internal_type.strip_prefix("struct ") {
                    if seen.insert(struct_name.to_string()) {
                        if let Some(components) = param.get("components").and_then(Value::as_array)
                        {
                            let fields = components
                                .iter()
                                .map(|field| {
                                    let field_name = field["name"].as_str().unwrap_or("_");
                                    let field_type = format_sol_type(field);
                                    format!("        {field_type} {field_name};")
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            structs
                                .push(format!("    struct {struct_name} {{\n{fields}\n    }}"));

                            // Recursively collect nested structs
                            collect_structs(components, seen, structs);
                        }
                    }
                }
            }
        } else if param["type"].as_str().map(|t| t.ends_with("[]")).unwrap_or(false) {
            // For arrays, check the base type
            if let Some(components) = param.get("components").and_then(Value::as_array) {
                collect_structs(components, seen, structs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use serde_json::json;

    #[test]
    fn test_simple_erc20_interface() {
        let abi = vec![
            json!({
                "name": "transfer",
                "type": "function",
                "inputs": [
                    {"name": "to", "type": "address", "internalType": "address"},
                    {"name": "amount", "type": "uint256", "internalType": "uint256"}
                ],
                "outputs": [{"name": "", "type": "bool", "internalType": "bool"}],
                "stateMutability": "nonpayable"
            }),
            json!({
                "name": "balanceOf",
                "type": "function",
                "inputs": [
                    {"name": "account", "type": "address", "internalType": "address"}
                ],
                "outputs": [{"name": "", "type": "uint256", "internalType": "uint256"}],
                "stateMutability": "view"
            }),
            json!({
                "name": "approve",
                "type": "function",
                "inputs": [
                    {"name": "spender", "type": "address", "internalType": "address"},
                    {"name": "amount", "type": "uint256", "internalType": "uint256"}
                ],
                "outputs": [{"name": "", "type": "bool", "internalType": "bool"}],
                "stateMutability": "nonpayable"
            }),
        ];

        let interface = generate("ERC20Token", &abi).unwrap();
        assert_snapshot!("erc20_interface", interface);
    }

    #[test]
    fn test_complex_structs_interface() {
        let abi = vec![json!({
            "name": "submitOrder",
            "type": "function",
            "inputs": [{
                "name": "order",
                "type": "tuple",
                "internalType": "struct Order",
                "components": [
                    {"name": "id", "type": "uint256", "internalType": "uint256"},
                    {"name": "user", "type": "address", "internalType": "address"},
                    {
                        "name": "items",
                        "type": "tuple[]",
                        "internalType": "struct Item[]",
                        "components": [
                            {"name": "productId", "type": "uint256", "internalType": "uint256"},
                            {"name": "quantity", "type": "uint256", "internalType": "uint256"},
                            {"name": "price", "type": "uint256", "internalType": "uint256"}
                        ]
                    },
                    {"name": "metadata", "type": "bytes", "internalType": "bytes"}
                ]
            }],
            "outputs": [{"name": "success", "type": "bool", "internalType": "bool"}],
            "stateMutability": "payable"
        })];

        let interface = generate("OrderManager", &abi).unwrap();
        assert_snapshot!("complex_structs_interface", interface);
    }

    #[test]
    fn test_all_function_mutabilities() {
        let abi = vec![
            json!({
                "name": "pureFunction",
                "type": "function",
                "inputs": [{"name": "x", "type": "uint256", "internalType": "uint256"}],
                "outputs": [{"name": "", "type": "uint256", "internalType": "uint256"}],
                "stateMutability": "pure"
            }),
            json!({
                "name": "viewFunction",
                "type": "function",
                "inputs": [],
                "outputs": [{"name": "", "type": "string", "internalType": "string"}],
                "stateMutability": "view"
            }),
            json!({
                "name": "payableFunction",
                "type": "function",
                "inputs": [{"name": "data", "type": "bytes", "internalType": "bytes"}],
                "outputs": [],
                "stateMutability": "payable"
            }),
            json!({
                "name": "nonpayableFunction",
                "type": "function",
                "inputs": [],
                "outputs": [],
                "stateMutability": "nonpayable"
            }),
        ];

        let interface = generate("MixedContract", &abi).unwrap();
        assert_snapshot!("all_mutabilities_interface", interface);
    }

    #[test]
    fn test_arrays_and_complex_types() {
        let abi = vec![json!({
            "name": "processData",
            "type": "function",
            "inputs": [
                {
                    "name": "addresses",
                    "type": "address[]",
                    "internalType": "address[]"
                },
                {
                    "name": "amounts",
                    "type": "uint256[]",
                    "internalType": "uint256[]"
                },
                {
                    "name": "data",
                    "type": "string",
                    "internalType": "string"
                },
                {
                    "name": "rawBytes",
                    "type": "bytes",
                    "internalType": "bytes"
                }
            ],
            "outputs": [
                {
                    "name": "results",
                    "type": "tuple[]",
                    "internalType": "struct Result[]",
                    "components": [
                        {"name": "addr", "type": "address", "internalType": "address"},
                        {"name": "value", "type": "uint256", "internalType": "uint256"}
                    ]
                }
            ],
            "stateMutability": "nonpayable"
        })];

        let interface = generate("DataProcessor", &abi).unwrap();
        assert_snapshot!("arrays_and_complex_types", interface);
    }

    #[test]
    fn test_empty_abi_interface() {
        let abi = vec![];
        let interface = generate("EmptyContract", &abi).unwrap();
        assert_snapshot!("empty_abi_interface", interface);
    }

    #[test]
    fn test_nested_structs_interface() {
        let abi = vec![json!({
            "name": "updateConfig",
            "type": "function",
            "inputs": [{
                "name": "config",
                "type": "tuple",
                "internalType": "struct Config",
                "components": [
                    {"name": "version", "type": "uint256", "internalType": "uint256"},
                    {
                        "name": "settings",
                        "type": "tuple",
                        "internalType": "struct Settings",
                        "components": [
                            {"name": "maxUsers", "type": "uint256", "internalType": "uint256"},
                            {"name": "timeout", "type": "uint256", "internalType": "uint256"},
                            {
                                "name": "permissions",
                                "type": "tuple",
                                "internalType": "struct Permissions",
                                "components": [
                                    {"name": "canRead", "type": "bool", "internalType": "bool"},
                                    {"name": "canWrite", "type": "bool", "internalType": "bool"}
                                ]
                            }
                        ]
                    }
                ]
            }],
            "outputs": [],
            "stateMutability": "nonpayable"
        })];

        let interface = generate("ConfigManager", &abi).unwrap();
        assert_snapshot!("nested_structs_interface", interface);
    }
}
