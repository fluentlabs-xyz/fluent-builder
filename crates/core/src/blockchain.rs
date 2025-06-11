//! Blockchain interaction utilities for contract verification

use serde::{Deserialize, Serialize};

/// Network configuration for blockchain interaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network name (e.g., "mainnet", "testnet", "local")
    pub name: String,
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Chain ID
    pub chain_id: u64,
}

impl NetworkConfig {
    /// Create configuration for local development network
    pub fn local() -> Self {
        Self {
            name: "local".to_string(),
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 1337,
        }
    }
    
    /// Create configuration for Fluent dev network
    pub fn fluent_dev() -> Self {
        Self {
            name: "fluent-dev".to_string(),
            rpc_url: "https://rpc.dev.gblend.xyz".to_string(),
            chain_id: 20993,
        }
    }
    
    /// Create custom network configuration
    pub fn custom(name: impl Into<String>, rpc_url: impl Into<String>, chain_id: u64) -> Self {
        Self {
            name: name.into(),
            rpc_url: rpc_url.into(),
            chain_id,
        }
    }
}

/// Contract deployment information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployedContractInfo {
    /// Contract address
    pub address: String,
    /// SHA256 hash of the deployed bytecode
    pub bytecode_hash: String,
    /// Size of bytecode in bytes
    pub bytecode_size: usize,
    /// Network where contract is deployed
    pub network: NetworkConfig,
}

#[cfg(feature = "ethers")]
pub mod ethers {
    use crate::utils;

    use super::*;
    use ::ethers::{
        providers::{Http, Middleware, Provider},
        types::Address,
    };
    use eyre::{Context, Result};
    
    /// Fetch deployed bytecode hash from blockchain
    pub async fn fetch_bytecode_hash(
        network: &NetworkConfig,
        contract_address: &str,
    ) -> Result<String> {
        let provider = Provider::<Http>::try_from(&network.rpc_url)
            .context("Failed to create provider")?;
        
        // Verify chain ID
        let chain_id = provider.get_chainid().await
            .context("Failed to get chain ID")?;
        
        if chain_id.as_u64() != network.chain_id {
            return Err(eyre::eyre!(
                "Chain ID mismatch: expected {}, got {}",
                network.chain_id,
                chain_id
            ));
        }
        
        // Parse address
        let address: Address = contract_address.parse()
            .context("Invalid contract address")?;
        
        // Get bytecode
        let bytecode = provider
            .get_code(address, None)
            .await
            .context("Failed to fetch contract bytecode")?;
        
        if bytecode.is_empty() {
            return Err(eyre::eyre!("No bytecode found at address {}", contract_address));
        }
        
        // Calculate and return hash
        let hash = utils::hash_bytes(&bytecode);
        Ok(format!("0x{}", hash))
    }
    
    /// Fetch full deployed contract information
    pub async fn fetch_deployed_contract_info(
        network: &NetworkConfig,
        contract_address: &str,
    ) -> Result<DeployedContractInfo> {
        let provider = Provider::<Http>::try_from(&network.rpc_url)
            .context("Failed to create provider")?;
        
        // Verify chain ID
        let chain_id = provider.get_chainid().await
            .context("Failed to get chain ID")?;
        
        if chain_id.as_u64() != network.chain_id {
            return Err(eyre::eyre!(
                "Chain ID mismatch: expected {}, got {}",
                network.chain_id,
                chain_id
            ));
        }
        
        // Parse address
        let address: Address = contract_address.parse()
            .context("Invalid contract address")?;
        
        // Get bytecode
        let bytecode = provider
            .get_code(address, None)
            .await
            .context("Failed to fetch contract bytecode")?;
        
        if bytecode.is_empty() {
            return Err(eyre::eyre!("No bytecode found at address {}", contract_address));
        }
        
        let bytecode_size = bytecode.len();
        let bytecode_hash = format!("0x{}", utils::hash_bytes(&bytecode));
        
        Ok(DeployedContractInfo {
            address: contract_address.to_string(),
            bytecode_hash,
            bytecode_size,
            network: network.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_network_configs() {
        let local = NetworkConfig::local();
        assert_eq!(local.chain_id, 1337);
        assert_eq!(local.rpc_url, "http://localhost:8545");
        
        let dev = NetworkConfig::fluent_dev();
        assert_eq!(dev.chain_id, 20993);
        assert_eq!(dev.name, "fluent-dev");
        
        let custom = NetworkConfig::custom("test", "http://test.com", 123);
        assert_eq!(custom.name, "test");
        assert_eq!(custom.chain_id, 123);
    }
}