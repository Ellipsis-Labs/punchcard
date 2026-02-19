# Punchcard

A native Solana program for tracking claimed indices using a bit vector.

## Overview

Punchcard creates on-chain accounts that track N claimable slots using a compact bit representation. Each slot can only be claimed once by the designated authority. When all slots are claimed, the account is automatically closed and rent is returned.

## Program ID

```
pcWKVSdcdDUKabPz4pVfaQ2jMod1kWv3LqeQivjKXiF
```

## Instructions

### Create

Creates a new punchcard account with the specified capacity.

**Accounts:**
| Index | Writable | Signer | Description |
|-------|----------|--------|-------------|
| 0 | Yes | Yes | Payer (becomes authority) |
| 1 | Yes | Yes | Punchcard account |
| 2 | No | No | System program |

**Data:**
```rust
Create { capacity: u64 }
```

### Claim

Claims one or more indices on the punchcard. Only the authority can claim. Fails if any index is already claimed or out of bounds. Closes the account when all indices are claimed.

**Accounts:**
| Index | Writable | Signer | Description |
|-------|----------|--------|-------------|
| 0 | Yes | Yes | Authority |
| 1 | Yes | No | Punchcard account |

**Data:**
```rust
Claim { indices: Vec<u64> }
```

## Account Structure

| Field     | Size                     |
|-----------|--------------------------|
| authority | 32 bytes                 |
| capacity  | 8 bytes                  |
| claimed   | 8 bytes                  |
| bits      | ceil(capacity / 8) bytes |

## Errors

| Code | Name | Description |
|------|------|-------------|
| 0 | InvalidAuthority | Signer does not match punchcard authority |
| 1 | IndexOutOfBounds | Index >= capacity |
| 2 | AlreadyClaimed | Index has already been claimed |

## Building

```bash
cargo build-sbf
```

## Testing

```bash
cargo test-sbf
```

## Dependencies

- [pinocchio](https://crates.io/crates/pinocchio) - Lightweight Solana program framework
- [borsh](https://crates.io/crates/borsh) - Instruction serialization
- [bytemuck](https://crates.io/crates/bytemuck) - Safe zero-copy casting
- [litesvm](https://crates.io/crates/litesvm) - Integration testing (dev)

## Verifiable build
```
solana-verify build -b solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8
```
