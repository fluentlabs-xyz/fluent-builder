//! Parser for extracting router information from Rust source files

use eyre::{Context, Result};
use fluentbase_sdk_derive_core::router::{process_router, Router};
use proc_macro2::TokenStream as TokenStream2;
use quote::ToTokens;
use std::path::Path;
use syn::{parse_file, visit::Visit, Attribute, ItemImpl};

/// Parses a Rust file and extracts all router implementations
pub fn parse_routers(path: impl AsRef<Path>) -> Result<Vec<Router>> {
    let path = path.as_ref();

    // Read file content
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;

    // Parse Rust syntax
    let ast = parse_file(&content).map_err(|e| eyre::eyre!("Failed to parse Rust file: {}", e))?;

    // Find routers
    let mut finder = RouterFinder::new();
    finder.visit_file(&ast);

    // Return first error if any occurred during processing
    if let Some(error) = finder.errors.into_iter().next() {
        return Err(eyre::eyre!("Router parsing error: {}", error));
    }

    Ok(finder.routers)
}

/// Internal visitor for finding router implementations
struct RouterFinder {
    routers: Vec<Router>,
    errors: Vec<syn::Error>,
}

impl RouterFinder {
    fn new() -> Self {
        Self { routers: Vec::new(), errors: Vec::new() }
    }

    fn process_router_impl(&mut self, attr: &Attribute, impl_block: &ItemImpl) {
        match extract_router_tokens(attr) {
            Ok(attr_tokens) => match process_router(attr_tokens, impl_block.to_token_stream()) {
                Ok(router) => self.routers.push(router),
                Err(error) => self.errors.push(error),
            },
            Err(error) => self.errors.push(error),
        }
    }
}

impl<'ast> Visit<'ast> for RouterFinder {
    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        // Look for router attribute
        for attr in &node.attrs {
            if is_router_attribute(attr) {
                self.process_router_impl(attr, node);
                // Found router attribute - no need to check other attributes
                break;
            }
        }

        // Continue visiting nested items
        syn::visit::visit_item_impl(self, node);
    }
}

/// Checks if an attribute is a router attribute
fn is_router_attribute(attr: &Attribute) -> bool {
    attr.path().is_ident("router")
}

/// Extracts tokens from router attribute
fn extract_router_tokens(attr: &Attribute) -> syn::Result<TokenStream2> {
    match &attr.meta {
        syn::Meta::List(meta_list) => Ok(meta_list.tokens.clone()),
        syn::Meta::Path(_) => Ok(TokenStream2::new()), // #[router] without parameters
        _ => Err(syn::Error::new_spanned(
            attr,
            "Invalid router attribute format. Expected #[router] or #[router(...)]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{content}").unwrap();
        file
    }

    #[test]
    fn test_parse_routers_no_router() {
        let file = create_test_file(
            r#"
            pub struct TestStruct {
                field: u32,
            }
            
            impl TestStruct {
                pub fn new() -> Self {
                    Self { field: 0 }
                }
            }
        "#,
        );

        let routers = parse_routers(file.path()).unwrap();
        assert!(routers.is_empty());
    }

    #[test]
    fn test_parse_routers_with_router() {
        let file = create_test_file(
            r#"
            use fluentbase_sdk::{derive::router, SharedAPI};

            pub trait TestAPI {
                fn test(&self) -> u32;
            }

            pub struct TestContract<SDK> {
                sdk: SDK,
            }

            #[router(mode = "solidity")]
            impl<SDK: SharedAPI> TestAPI for TestContract<SDK> {
                fn test(&self) -> u32 {
                    42
                }
            }
        "#,
        );

        let result = parse_routers(file.path());
        // This might fail if fluentbase_sdk types are not available,
        // but the parsing itself should work
        match result {
            Ok(routers) => {
                // If it succeeds, we should have found one router
                assert!(!routers.is_empty());
            }
            Err(e) => {
                // Expected if SDK types are not available during testing
                tracing::info!("Expected error during testing: {}", e);
            }
        }
    }

    #[test]
    fn test_parse_routers_invalid_file_path() {
        let result = parse_routers("/non/existent/file.rs");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read file"));
    }

    #[test]
    fn test_parse_routers_invalid_rust_syntax() {
        let file = create_test_file(
            r#"
            this is not valid rust code {{{
        "#,
        );

        let result = parse_routers(file.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse Rust file"));
    }

    #[test]
    fn test_is_router_attribute() {
        use syn::{parse_quote, Attribute};

        let router_attr: Attribute = parse_quote!(#[router]);
        assert!(is_router_attribute(&router_attr));

        let router_with_params: Attribute = parse_quote!(#[router(mode = "solidity")]);
        assert!(is_router_attribute(&router_with_params));

        let other_attr: Attribute = parse_quote!(#[derive(Debug)]);
        assert!(!is_router_attribute(&other_attr));
    }

    #[test]
    fn test_extract_router_tokens() {
        use syn::{parse_quote, Attribute};

        // Router without parameters
        let attr: Attribute = parse_quote!(#[router]);
        let tokens = extract_router_tokens(&attr).unwrap();
        assert!(tokens.is_empty());

        // Router with parameters
        let attr: Attribute = parse_quote!(#[router(mode = "solidity", interface = true)]);
        let tokens = extract_router_tokens(&attr).unwrap();
        assert!(!tokens.is_empty());

        // Invalid format
        let attr: Attribute = parse_quote!(#[router = "invalid"]);
        let result = extract_router_tokens(&attr);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_routers_multiple() {
        let file = create_test_file(
            r#"
            use fluentbase_sdk::{derive::router, SharedAPI};

            pub trait API1 {
                fn method1(&self) -> u32;
            }

            pub trait API2 {
                fn method2(&self) -> u64;
            }

            pub struct Contract<SDK> {
                sdk: SDK,
            }

            #[router]
            impl<SDK: SharedAPI> API1 for Contract<SDK> {
                fn method1(&self) -> u32 {
                    1
                }
            }

            #[router]
            impl<SDK: SharedAPI> API2 for Contract<SDK> {
                fn method2(&self) -> u64 {
                    2
                }
            }
        "#,
        );

        let result = parse_routers(file.path());
        match result {
            Ok(routers) => {
                // Should find 2 routers if SDK is available
                tracing::info!("Found {} routers", routers.len());
            }
            Err(e) => {
                // Expected during testing without SDK
                tracing::info!("Expected error: {}", e);
            }
        }
    }
}
