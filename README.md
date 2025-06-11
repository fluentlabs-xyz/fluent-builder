# Fluent Compiler Suite

[![Build Status](https://github.com/fluentlabs-xyz/fluent-compiler/actions/workflows/ci.yml/badge.svg)](https://github.com/fluentlabs-xyz/fluent-compiler/actions)

A Rust library and command-line interface (CLI) for compiling and verifying Rust smart contracts for the Fluent blockchain.

## Features

- **Dual-Format Compilation**: Compiles Rust contracts to both `WASM` and the execution-optimized `rWASM` format.
- **Automatic ABI Generation**: Extracts a Solidity-compatible ABI and interface from `#[router]` macros.
- **Reproducible Builds**: Enforces verifiability by default through Git, with a flexible fallback mode for local development.
- **On-Chain Verification**: Provides a high-level `verify` function to match local source code against the bytecode of a deployed contract.

---

## Quick Start: Compile & Deploy in 90 Seconds

This guide will walk you through compiling, deploying, and verifying an example contract using a single command.

**Prerequisites:**

- Rust & Cargo.
- An environment variable `DEPLOY_PRIVATE_KEY` with a funded private key (e.g., `export DEPLOY_PRIVATE_KEY=0x...`).

**Steps:**

1. **Clone the Repository**

    ```bash
    git clone https://github.com/fluentlabs-xyz/fluent-compiler.git
    cd fluent-compiler
    ```

2. **Install Rust WASM Target**

    ```bash
    rustup target add wasm32-unknown-unknown
    ```

3. **Compile, Deploy, and Verify**
    This project uses `just` as a command runner. The following command runs the entire workflow for the `power-calculator` example. You will need `gblend` installed for the deployment step.

    ```bash
    just deploy-and-verify
    ```

    This command automates the entire process:
    - Builds the `fluent-compiler` CLI.
    - Compiles the example contract.
    - Deploys it to the Fluent testnet using `gblend`.
    - Verifies that the deployed bytecode matches the local source code.

You have just completed a full, verifiable deployment!

---

## CLI Usage

The `fluent-compiler` binary is the primary way to interact with the toolkit.

### `compile`

The `compile` command builds your contract. It operates in two distinct modes to ensure your builds are always reproducible.

#### 1. Git Source (Default Mode)

This is the **default and recommended** mode for official builds. It requires your project to be in a clean Git repository (no uncommitted changes).

```bash
# Fails if the repository has uncommitted changes
fluent-compiler compile ./path/to/my-contract
```

#### 2. Archive Source (Fallback Mode)

Use the `--allow-dirty` flag to bypass the Git check. This is ideal for local development.

```bash
# Compiles even with uncommitted changes
fluent-compiler compile ./path/to/my-contract --allow-dirty
```

### `verify`

The `verify` command checks if a deployed contract matches your local source code.

```bash
fluent-compiler verify ./path/to/my-contract \
  --address 0x1234... \
  --chain-id 20993 \
  --rpc https://rpc.dev.gblend.xyz
```

---

## Development with `just`

We use `just` to automate common development tasks.

| Command | Description |
| :--- | :--- |
| `just compile` | Builds the `fluent-compiler` CLI binary. |
| `just link` | Builds the CLI and creates a symlink (`./fluent-compiler`) for easy local use. |
| `just compile-examples` | Compiles an example contract. Pass flags like `just compile-examples --allow-dirty`. |
| `just deploy-and-verify` | **(Most useful command)** Runs the full compile, deploy, and verify workflow. |
| `just test` | Runs all tests in the workspace. |
| `just clean` | Deletes build artifacts and generated files. |
