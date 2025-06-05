# fluent-compiler-cli

**Purpose:** A command-line interface for compiling Rust smart contracts to WASM/rWASM bytecode for the Fluent blockchain. By default, it compiles and saves all artifacts to disk, making it ideal for development workflows while also supporting CI/CD pipelines.

**Key Features:**
• Compiles Rust contracts to both WASM and rWASM formats with full metadata generation
• Saves all compilation artifacts by default (WASM, rWASM, ABI, interface, metadata)
• Supports JSON-only output mode for CI/CD pipelines and automation
• Creates source archives for reproducible builds and verification
• Configurable via JSON files with CLI argument overrides

**Basic Usage:**

```bash
# Compile current directory and save all artifacts to ./out
fluent-compiler

# Compile a specific project
fluent-compiler ./my-contract

# Specify custom output directory
fluent-compiler -o ./build

# Output only JSON to stdout (no files saved)
fluent-compiler --json-only

# Create source archive for verification
fluent-compiler --archive

# Use configuration file
fluent-compiler -c config.json

# Verbose logging
fluent-compiler -v

# Quiet mode (errors only)
fluent-compiler -q

# Compact JSON output
fluent-compiler --json-only --compact
```

**Output Modes:**

1. **Default mode** (saves files):

```bash
$ fluent-compiler
✅ Successfully compiled my-contract
📁 Output directory: out/my-contract.wasm
📄 Created files:
   - lib.wasm
   - lib.rwasm
   - abi.json
   - interface.sol
   - metadata.json
```

2. **JSON-only mode** (for pipelines):

```bash
$ fluent-compiler --json-only
{
  "contract_name": "my-contract",
  "wasm_bytecode_hex": "0061736d01000000...",
  "rwasm_bytecode_hex": "00525753...",
  "abi": [...],
  "build_metadata": {
    "compiler": {
      "name": "rustc",
      "version": "rustc 1.75.0 (82e1608df 2023-12-21)"
    },
    "settings": {
      "target_triple": "wasm32-unknown-unknown",
      "profile": "release"
    }
  }
}
```

**Directory Structure:**

When saving artifacts (default behavior), creates:

```
out/
└── my-contract.wasm/
    ├── lib.wasm         # WASM bytecode
    ├── lib.rwasm        # rWASM bytecode
    ├── abi.json         # Solidity ABI
    ├── interface.sol    # Solidity interface
    ├── metadata.json    # Build metadata
    └── sources.tar.gz   # Source archive (if --archive)
```

**Configuration File:**

Create a `config.json` to customize compilation:

```json
{
  "project_root": "./contracts/my-contract",
  "output_dir": "./build",
  "wasm": {
    "target": "wasm32-unknown-unknown",
    "profile": "Release",
    "features": ["production"],
    "no_default_features": true
  },
  "artifacts": {
    "generate_interface": true,
    "generate_abi": true,
    "generate_metadata": true,
    "pretty_json": true
  }
}
```

**CI/CD Integration:**

```bash
# Extract specific values using jq
fluent-compiler --json-only | jq -r '.rwasm_bytecode_hex'

# Validate compilation in CI
fluent-compiler --json-only -q || exit 1

# Archive sources for verification
fluent-compiler --archive -o ./verification-bundle
```

**Error Handling:**

Errors are output as JSON to stderr:

```json
{
  "error": "Compilation failed",
  "details": "No fluentbase-sdk dependency found"
}
```

**Requirements:**

- Rust toolchain with `wasm32-unknown-unknown` target
- Valid Fluent contract with `fluentbase-sdk` dependency
